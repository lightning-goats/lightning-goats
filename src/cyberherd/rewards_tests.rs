//! Real temporary ledgers/private framing with synthetic settlements only.
use anyhow::Result;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
};
use uuid::Uuid;

use crate::{
    core_bridge::{
        AuthenticatedAllowance, AuthenticatedReservation, Response,
        unix::{Client, self_credentials},
    },
    cyberherd::{
        calendar::{self, DayWindow},
        discovery::Discovery,
        ingress::CoreBinding,
        membership::{Action, FeederAvailability, Settings, Target},
        relay::{Filter, Query, ReadBatch},
        rewards::{PlanningPolicy, ProfileRead, Skip, State},
        store::{Interaction, Store},
        targets::{AuthenticatedProfile, TargetPolicy, Targets},
        worker::{self, Environment},
    },
    domain::payment::SettledPayment,
    ledger::{LedgerStore, OperatorCreditGrant},
    nostr_event::{VerifiedEvent, test_event},
    payout::recipient::Recipient,
    reward_funding::{Allowance, Reservation, RewardPolicy},
};

#[path = "payout_bridge_tests.rs"]
mod payout_bridge_tests;

fn key(n: u8) -> String {
    test_event(n, 0, 100, "{}", vec![]).pubkey
}
fn profile(n: u8, at: i64, address: Option<&str>) -> AuthenticatedProfile {
    AuthenticatedProfile::parse(
        VerifiedEvent::verify(test_event(
            n,
            0,
            at as u64,
            &json!({"name":format!("Member {n}"),"lud16":address}).to_string(),
            vec![],
        ))
        .unwrap(),
        &key(n),
        at,
    )
    .unwrap()
}
fn policy() -> PlanningPolicy {
    PlanningPolicy {
        fee_basis_points: 0,
        minimum_fee_sats: 1,
        minimum_net_sats: 1,
        profile_max_age_seconds: 3600,
        operation_addresses: vec!["herd@lightning-goats.com".into()],
    }
}
fn terms(address: &str) -> Recipient {
    let user = address.split_once('@').unwrap().0;
    Recipient::parse(address, &serde_json::to_vec(&json!({"tag":"payRequest",
        "callback":format!("https://pay.example.com/{user}"), "minSendable":1,"maxSendable":100000000,
        "metadata":"[[\"text/plain\",\"treat\"]]"})).unwrap()).unwrap()
}

async fn peer(response: Response) -> (TempDir, Client, tokio::task::JoinHandle<()>) {
    let root = TempDir::new().unwrap();
    let path = root.path().join("core.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let length = socket.read_u32().await.unwrap();
        let mut bytes = vec![0; length as usize];
        socket.read_exact(&mut bytes).await.unwrap();
        let raw = serde_json::to_vec(&response).unwrap();
        socket.write_u32(raw.len() as u32).await.unwrap();
        socket.write_all(&raw).await.unwrap();
    });
    let client = Client::new(path, self_credentials().unwrap().0).unwrap();
    (root, client, task)
}
async fn authenticated(a: Allowance) -> AuthenticatedAllowance {
    let scope = (a.epoch, a.request_id, a.owner.clone());
    let (_root, client, task) = peer(Response::Allowance {
        allowance: Box::new(a),
    })
    .await;
    let result = client
        .reward_allowance(scope.0, scope.1, &scope.2)
        .await
        .unwrap();
    task.await.unwrap();
    result
}
async fn approved(r: Reservation) -> AuthenticatedReservation {
    let (_root, client, task) = peer(Response::Reservation {
        reservation: Box::new(r.clone()),
    })
    .await;
    let result = client
        .reserve_reward(
            r.epoch,
            r.request_id,
            &r.owner,
            &r.plan_hash,
            r.reserved_sats,
        )
        .await
        .unwrap();
    task.await.unwrap();
    result
}

struct Fixture {
    _root: TempDir,
    core: LedgerStore,
    store: Store,
    url: String,
    now: i64,
    id: Uuid,
    owner: String,
    root_policy: RewardPolicy,
}
impl Fixture {
    async fn new(virtual_sats: u64) -> Self {
        Self::with_conversations(virtual_sats, false).await
    }
    async fn with_conversations(virtual_sats: u64, with_conversations: bool) -> Self {
        let root = TempDir::new().unwrap();
        let core = LedgerStore::connect(&format!(
            "sqlite://{}",
            root.path().join("core.db").display()
        ))
        .await
        .unwrap();
        core.initialize_reward_funding().await.unwrap();
        let now = calendar::now().unwrap();
        if virtual_sats > 0 {
            core.grant_operator_credit(
                &OperatorCreditGrant {
                    id: "virtual-fixture".into(),
                    amount_sats: virtual_sats,
                    address_user: "herd".into(),
                    reason: "Synthetic test credit".into(),
                },
                false,
            )
            .await
            .unwrap();
        }
        if virtual_sats < 1000 {
            core.record_payment(&SettledPayment {
                source: "strike".into(),
                source_id: "synthetic".into(),
                payment_hash: None,
                address_user: "herd".into(),
                credit_pool: "herd".into(),
                amount_msat: (1000 - virtual_sats) * 1000,
                settled_at: Some(now),
                context_json: None,
            })
            .await
            .unwrap();
        }
        let owner = key(1);
        let url = format!("sqlite://{}", root.path().join("herd.db").display());
        let store = Store::connect(&url).await.unwrap();
        store
            .initialize(
                &owner,
                &Settings {
                    minimum_sats: 10,
                    max_members: 10,
                    member_percent: 10,
                },
                now - 100,
            )
            .await
            .unwrap();
        let epoch = core.overlay_stream_id().await.unwrap();
        store
            .initialize_core_ingress(
                &owner,
                &CoreBinding {
                    core_uid: self_credentials().unwrap().0,
                    epoch,
                    start_after: 0,
                    credit_pool: "herd".into(),
                    payees: BTreeMap::from([("herd".into(), owner.clone())]),
                },
            )
            .await
            .unwrap();
        for (n, action) in [
            (2, Action::Zap { amount_sats: 10 }),
            (3, Action::Repost),
            (4, Action::Like),
        ] {
            let context = if with_conversations {
                Some(serde_json::from_value::<super::targets::InteractionContext>(json!({
                    "matched":{"event_id":"66".repeat(32),"kind":1,"coordinate":null,"payee_pubkey":owner},
                    "destinations":[{"event_id":"66".repeat(32),"kind":1,"coordinate":null,"payee_pubkey":owner},
                    {"event_id":"77".repeat(32),"kind":30311,"coordinate":format!("30311:{owner}:original"),"payee_pubkey":owner}]
                })).unwrap())
            } else {
                None
            };
            let address = format!("member{n}@example.com");
            let p = profile(n, now - 2, Some(&address));
            let source_id = if n == 2 {
                "settlement:fixture".into()
            } else {
                format!("nostr:{}", format!("{n:02x}").repeat(32))
            };
            store
                .process_with_context(
                    &owner,
                    &Interaction {
                        source_id,
                        source_hash: format!("{n:02x}").repeat(32),
                        pubkey: key(n),
                        profile: p.profile().clone(),
                        action,
                        occurred_at: now - 1,
                    },
                    Target::Current,
                    FeederAvailability::Ready,
                    context.as_ref(),
                )
                .await
                .unwrap();
            store.remember_profile(&owner, &p).await.unwrap();
            store
                .record_reward_profile_read(&owner, &key(n), true, now)
                .await
                .unwrap();
        }
        let id = core.begin_feed_attempt(1000).await.unwrap().unwrap();
        core.confirm_feed_attempt(id).await.unwrap();
        let now = calendar::now().unwrap();
        let batch = core.core_event_batch(Some(epoch), 0, 128).await.unwrap();
        let (_peer, client, task) = peer(Response::Events {
            batch: Box::new(batch),
        })
        .await;
        let batch = client.events(Some(epoch), 0, 128).await.unwrap();
        task.await.unwrap();
        store.ingest_core(&owner, &batch).await.unwrap();
        let targets = Targets::project(
            TargetPolicy {
                project_pubkey: owner.clone(),
                tracked_tags: vec!["CyberHerd".into()],
            },
            &DayWindow::at("America/Denver", now).unwrap(),
            vec![],
            vec![],
            BTreeSet::new(),
            now,
        )
        .unwrap();
        while store.ingress_head(&owner).await.unwrap().is_some() {
            worker::step(
                &store,
                &owner,
                Environment {
                    targets: &targets,
                    discovery_available: true,
                    feeder: FeederAvailability::Ready,
                    profile: None,
                    now,
                },
            )
            .await
            .unwrap();
        }
        let root_policy = RewardPolicy {
            owner: owner.clone(),
            member_percent_ceiling: 10,
            max_sats_per_feed: 100,
            max_sats_per_24h: 100,
        };
        Self {
            _root: root,
            core,
            store,
            url,
            now,
            id,
            owner,
            root_policy,
        }
    }
    async fn allowance(&self) -> AuthenticatedAllowance {
        authenticated(
            self.core
                .reward_allowance(
                    &self.root_policy,
                    self.core.overlay_stream_id().await.unwrap(),
                    self.id,
                )
                .await
                .unwrap(),
        )
        .await
    }
    async fn reads(&self) -> Vec<ProfileRead> {
        let mut profiles = vec![];
        for n in [2, 3, 4] {
            profiles.push(
                self.store
                    .reward_profile_read(&self.owner, &key(n), self.now, 3600)
                    .await
                    .unwrap()
                    .unwrap(),
            );
        }
        profiles
    }
    async fn freeze(&self, p: &PlanningPolicy) -> Result<super::rewards::StoredPlan> {
        let reads = self.reads().await;
        let terms: BTreeMap<_, _> = reads
            .iter()
            .filter_map(|p| p.address().map(|a| (p.pubkey().into(), Some(terms(a)))))
            .collect();
        self.store
            .freeze_reward_plan(
                &self.owner,
                self.id,
                super::rewards::Preparation {
                    policy: p,
                    allowance: Some(&self.allowance().await),
                    profiles: &reads,
                    recipients: &terms,
                    now: self.now,
                },
            )
            .await
    }
}

#[tokio::test]
async fn mixed_funding_freezes_whole_sat_shares_and_exact_reservation_across_restart() {
    let f = Fixture::new(300).await;
    let plan = f.freeze(&policy()).await.unwrap();
    assert_eq!(
        (
            plan.plan.real_sats,
            plan.plan.budget_sats,
            plan.plan.reserved_sats,
            plan.plan.operations_sats
        ),
        (700, 70, 68, 932)
    );
    // Six-decimal percentages are frozen before sat rounding: 70 * 2.142857
    // / 10 is 14.999999, so the repost share is 14 sats, not the full-rational
    // 15. Both that fraction and the zap fraction remain with operations.
    for (n, gross) in [(2, 49), (3, 14), (4, 5)] {
        let row = plan.plan.rows.iter().find(|r| r.pubkey == key(n)).unwrap();
        assert_eq!(
            (row.gross_sats, row.net_sats, row.fee_limit_sats),
            (gross, gross - 1, 1)
        );
    }
    let r = f
        .core
        .reserve_reward(
            &f.root_policy,
            plan.plan.epoch,
            f.id,
            &plan.hash,
            plan.plan.reserved_sats,
        )
        .await
        .unwrap();
    // A lost response and reopening preserve the exact plan, then the same core
    // reservation can be attached. No source/profile/fee refresh changes it.
    let reopened = Store::connect(&f.url).await.unwrap();
    assert_eq!(
        reopened.reward_plan(&f.owner, f.id).await.unwrap().unwrap(),
        plan
    );
    let r = approved(r).await;
    let attached = reopened
        .attach_reward_reservation(&f.owner, &r)
        .await
        .unwrap();
    assert_eq!(attached.state, State::Reserved);
    assert_eq!(
        reopened
            .attach_reward_reservation(&f.owner, &r)
            .await
            .unwrap(),
        attached
    );
    assert!(
        reopened
            .next_reward_feeding(&f.owner)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(f.core.feed_credit_sats().await.unwrap(), 0);
    assert_eq!(
        reopened
            .freeze_reward_plan(
                &f.owner,
                f.id,
                super::rewards::Preparation {
                    policy: &policy(),
                    allowance: None,
                    profiles: &[],
                    recipients: &BTreeMap::new(),
                    now: f.now + 999
                }
            )
            .await
            .unwrap(),
        attached
    );
}

#[tokio::test]
async fn current_signed_negative_and_failed_reads_do_not_reuse_historical_payout_addresses() {
    let f = Fixture::new(0).await;
    let old = f.reads().await;
    // Reserved domains must be a negative payout update, not pass profile
    // validation and then abort the whole plan in the stricter resolver.
    let negative = profile(2, f.now, Some("member@pay.test"));
    f.store.remember_profile(&f.owner, &negative).await.unwrap();
    assert!(
        f.store
            .reward_profile_read(&f.owner, &key(2), f.now, 3600)
            .await
            .unwrap()
            .is_none()
    );
    let terms = old
        .iter()
        .map(|p| (p.pubkey().into(), Some(terms(p.address().unwrap()))))
        .collect();
    assert!(
        f.store
            .freeze_reward_plan(
                &f.owner,
                f.id,
                super::rewards::Preparation {
                    policy: &policy(),
                    allowance: Some(&f.allowance().await),
                    profiles: &old,
                    recipients: &terms,
                    now: f.now
                }
            )
            .await
            .is_err()
    );
    f.store
        .record_reward_profile_read(&f.owner, &key(2), false, f.now)
        .await
        .unwrap();
    assert!(
        f.store
            .reward_profile_read(&f.owner, &key(2), f.now, 3600)
            .await
            .unwrap()
            .is_none()
    );
    f.store
        .record_reward_profile_read(&f.owner, &key(2), true, f.now)
        .await
        .unwrap();
    let plan = f.freeze(&policy()).await.unwrap();
    let row = plan.plan.rows.iter().find(|r| r.pubkey == key(2)).unwrap();
    assert_eq!(row.skip, Some(Skip::NoAddress));
    assert_eq!(row.gross_sats, 0);
    assert!(row.profile.as_ref().unwrap().lud16.is_none());
    // History is unchanged; it cannot override the newer signed negative head.
    assert!(
        f.store
            .feeding_barrier(&f.owner, f.id)
            .await
            .unwrap()
            .unwrap()
            .snapshot
            .roster
            .members[&key(2)]
            .profile
            .lud16
            .is_some()
    );
}

#[tokio::test]
async fn recipient_minimum_and_fetch_failure_leave_their_share_with_operations() {
    let f = Fixture::new(0).await;
    let reads = f.reads().await;
    let mut recipients: BTreeMap<_, _> = reads
        .iter()
        .map(|p| (p.pubkey().into(), Some(terms(p.address().unwrap()))))
        .collect();
    recipients
        .get_mut(&key(3))
        .unwrap()
        .as_mut()
        .unwrap()
        .min_msat = 21_000;
    recipients.insert(key(4), None);
    let plan = f
        .store
        .freeze_reward_plan(
            &f.owner,
            f.id,
            super::rewards::Preparation {
                policy: &policy(),
                allowance: Some(&f.allowance().await),
                profiles: &reads,
                recipients: &recipients,
                now: f.now,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (plan.plan.reserved_sats, plan.plan.operations_sats),
        (71, 929)
    );
    let row = plan.plan.rows.iter().find(|r| r.pubkey == key(3)).unwrap();
    assert_eq!(
        (row.gross_sats, row.net_sats, row.skip.clone()),
        (21, 0, Some(Skip::RecipientMinimum))
    );
    assert_eq!(
        plan.plan
            .rows
            .iter()
            .find(|r| r.pubkey == key(4))
            .unwrap()
            .skip,
        Some(Skip::RecipientUnavailable)
    );
}

#[tokio::test]
async fn shared_addresses_callbacks_and_operator_wallets_cannot_create_member_transfers() {
    let f = Fixture::new(0).await;
    for (n, address) in [
        (2, "Shared@example.com"),
        (3, "shared@EXAMPLE.COM"),
        (4, "herd@lightning-goats.com"),
    ] {
        f.store
            .remember_profile(&f.owner, &profile(n, f.now, Some(address)))
            .await
            .unwrap();
        f.store
            .record_reward_profile_read(&f.owner, &key(n), true, f.now)
            .await
            .unwrap();
    }
    let plan = f.freeze(&policy()).await.unwrap();
    assert_eq!((plan.plan.reserved_sats, plan.state), (0, State::NoPayout));
    assert!(
        plan.plan
            .rows
            .iter()
            .all(|r| matches!(r.skip, Some(Skip::SharedAddress | Skip::Operator)))
    );
    let g = Fixture::new(0).await;
    let reads = g.reads().await;
    let mut recipients: BTreeMap<_, _> = reads
        .iter()
        .map(|p| (p.pubkey().into(), Some(terms(p.address().unwrap()))))
        .collect();
    for recipient in recipients.values_mut() {
        recipient.as_mut().unwrap().callback = "https://pay.example.com/same-wallet".into();
    }
    let plan = g
        .store
        .freeze_reward_plan(
            &g.owner,
            g.id,
            super::rewards::Preparation {
                policy: &policy(),
                allowance: Some(&g.allowance().await),
                profiles: &reads,
                recipients: &recipients,
                now: g.now,
            },
        )
        .await
        .unwrap();
    assert_eq!(plan.plan.reserved_sats, 0);
    assert!(
        plan.plan
            .rows
            .iter()
            .all(|r| r.skip == Some(Skip::SharedCallback))
    );
}

#[tokio::test]
async fn virtual_and_zero_allocations_close_without_lookups_or_backpay() {
    for virtual_sats in [0, 1000] {
        let f = Fixture::new(virtual_sats).await;
        let mut allowance = f
            .core
            .reward_allowance(
                &f.root_policy,
                f.core.overlay_stream_id().await.unwrap(),
                f.id,
            )
            .await
            .unwrap();
        allowance.member_percent_ceiling = 0;
        let a = authenticated(allowance).await;
        let plan = f
            .store
            .freeze_reward_plan(
                &f.owner,
                f.id,
                super::rewards::Preparation {
                    policy: &policy(),
                    allowance: if virtual_sats == 0 { Some(&a) } else { None },
                    profiles: &[],
                    recipients: &BTreeMap::new(),
                    now: f.now,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            (
                plan.plan.reserved_sats,
                plan.plan.operations_sats,
                plan.state
            ),
            (0, 1000, State::NoPayout)
        );
        assert!(plan.plan.rows.is_empty());
        let later = f.freeze(&policy()).await.unwrap();
        assert_eq!(later.hash, plan.hash);
    }
}

#[tokio::test]
async fn fresh_lookup_completion_is_required_and_failed_quorum_invalidates_cached_success() {
    struct Source {
        profile: VerifiedEvent,
        available: bool,
    }
    impl Query for Source {
        async fn query(&self, _filters: &[Filter]) -> Result<ReadBatch> {
            Ok(ReadBatch {
                events: vec![self.profile.clone()],
                completed_relays: usize::from(self.available),
                failed_relays: usize::from(!self.available),
                saturated: false,
            })
        }
    }
    let f = Fixture::new(0).await;
    for available in [false, true] {
        let p = profile(2, f.now, None);
        let source = Source {
            profile: p.event().clone(),
            available,
        };
        Discovery::new(source, 1)
            .unwrap()
            .refresh_reward_profiles(&f.store, &f.owner, &[key(2)], f.now)
            .await
            .unwrap();
        let read = f
            .store
            .reward_profile_read(&f.owner, &key(2), f.now, 3600)
            .await
            .unwrap();
        assert_eq!(read.is_some(), available);
        if let Some(read) = read {
            assert!(read.address().is_none());
        }
    }
    assert!(
        f.store
            .reward_profile_read(&f.owner, &key(2), f.now + 3601, 3600)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        f.store
            .reward_profile_read(&f.owner, &key(2), f.now - 1, 3600)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn frozen_plans_are_atomic_owner_scoped_retained_and_cannot_exceed_root_limit() {
    let f = Fixture::new(0).await;
    let reads = f.reads().await;
    let terms = reads
        .iter()
        .map(|p| (p.pubkey().into(), Some(terms(p.address().unwrap()))))
        .collect();
    let mut a = f
        .core
        .reward_allowance(
            &f.root_policy,
            f.core.overlay_stream_id().await.unwrap(),
            f.id,
        )
        .await
        .unwrap();
    a.available_sats = 1;
    let a = authenticated(a).await;
    assert!(
        f.store
            .freeze_reward_plan(
                &f.owner,
                f.id,
                super::rewards::Preparation {
                    policy: &policy(),
                    allowance: Some(&a),
                    profiles: &reads,
                    recipients: &terms,
                    now: f.now
                }
            )
            .await
            .is_err()
    );
    assert!(f.store.reward_plan(&f.owner, f.id).await.unwrap().is_none());
    let plan = f.freeze(&policy()).await.unwrap();
    let pool = sqlx::SqlitePool::connect(&f.url).await.unwrap();
    assert!(
        sqlx::query("DELETE FROM reward_plans")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE reward_plans SET plan_hash='fake'")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(f.store.reward_plan(&key(9), f.id).await.unwrap().is_none());
    let mut changed = plan.plan.clone();
    changed.rows[0].net_sats += 1;
    assert_ne!(changed.hash().unwrap(), plan.hash);
}

#[tokio::test]
async fn core_allowance_remains_advisory_and_counts_existing_reservations() {
    let f = Fixture::new(0).await;
    let a = f.allowance().await;
    assert_eq!(a.allowance().available_sats, 100);
    let plan = f.freeze(&policy()).await.unwrap();
    f.core
        .reserve_reward(
            &f.root_policy,
            plan.plan.epoch,
            f.id,
            &plan.hash,
            plan.plan.reserved_sats,
        )
        .await
        .unwrap();
    assert_eq!(f.allowance().await.allowance().available_sats, 1);
    assert!(
        f.core
            .reward_allowance(&f.root_policy, Uuid::new_v4(), f.id)
            .await
            .is_err()
    );
    assert!(
        f.core
            .reward_allowance(&f.root_policy, plan.plan.epoch, Uuid::new_v4())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn bounded_worker_reserves_saved_plan_before_testing_available_budget() {
    struct NoRead;
    impl Query for NoRead {
        async fn query(&self, _: &[Filter]) -> Result<ReadBatch> {
            panic!("frozen retry must not refresh profiles")
        }
    }
    let f = Fixture::new(0).await;
    // Stage exact-head metadata as a previous bounded round would. The next
    // round freezes without starting an invoice/provider/signing request.
    for read in f.reads().await {
        f.store
            .stage_reward_recipient(&f.owner, f.id, &read, Some(&terms(read.address().unwrap())))
            .await
            .unwrap();
    }
    let a = f
        .core
        .reward_allowance(
            &f.root_policy,
            f.core.overlay_stream_id().await.unwrap(),
            f.id,
        )
        .await
        .unwrap();
    let (_root, client, task) = peer(Response::Allowance {
        allowance: Box::new(a),
    })
    .await;
    let runner = super::reward_worker::Worker {
        store: f.store.clone(),
        client,
        discovery: Discovery::new(NoRead, 1).unwrap(),
        owner: f.owner.clone(),
        policy: policy(),
    };
    assert_eq!(
        runner.tick().await.unwrap(),
        super::reward_worker::Step::Frozen
    );
    task.await.unwrap();
    drop(runner);
    let plan = f.store.reward_plan(&f.owner, f.id).await.unwrap().unwrap();
    let reservation = f
        .core
        .reserve_reward(
            &f.root_policy,
            plan.plan.epoch,
            f.id,
            &plan.hash,
            plan.plan.reserved_sats,
        )
        .await
        .unwrap();
    let (_root, client, task) = peer(Response::Reservation {
        reservation: Box::new(reservation.clone()),
    })
    .await;
    let runner = super::reward_worker::Worker {
        store: f.store.clone(),
        client,
        discovery: Discovery::new(NoRead, 1).unwrap(),
        owner: f.owner.clone(),
        policy: policy(),
    };
    assert_eq!(
        runner.tick().await.unwrap(),
        super::reward_worker::Step::Reserved
    );
    task.await.unwrap();
    assert_eq!(
        f.store
            .reward_plan(&f.owner, f.id)
            .await
            .unwrap()
            .unwrap()
            .reservation,
        Some(reservation)
    );
}

#[test]
fn fees_round_up_without_overspending_and_zero_settings_remain_zero() {
    let p = PlanningPolicy {
        fee_basis_points: 10001,
        ..policy()
    };
    assert!(p.validate().is_err());
    let p = PlanningPolicy {
        minimum_net_sats: 0,
        ..policy()
    };
    assert!(p.validate().is_err());
    let p = PlanningPolicy {
        minimum_fee_sats: u64::MAX,
        ..policy()
    };
    assert!(p.validate().is_err());
}
#[path = "result_tests.rs"]
mod result_tests;
