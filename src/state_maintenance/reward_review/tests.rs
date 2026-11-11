//! Native synthetic classification fixtures; protected backup/CLI authority is
//! separately exercised by the required root and installed systemd fixtures.
use super::*;
use crate::{
    core_bridge::EventBody,
    cyberherd::{
        membership::{Member, Profile},
        rewards::{PlanningPolicy, Row},
        shares::SocialAwards,
        store::{Snapshot, Store},
    },
    domain::payment::SettledPayment,
    ledger::{LedgerStore, OperatorCreditGrant},
    reward_funding::RewardPolicy,
};

struct Fixture {
    root: tempfile::TempDir,
    core: LedgerStore,
    member: Store,
    member_db: SqlitePool,
    policy: Policy,
    manifest: Manifest,
    after: u64,
}

fn key(n: u8) -> String {
    crate::nostr_event::test_event(n, 0, 100, "", vec![]).pubkey
}

#[test]
fn private_json_decode_errors_never_echo_record_values() {
    let error = decode::<Settings>(
        r#"{"minimum_sats":"private-record-marker","max_members":10,"member_percent":10}"#,
    )
    .unwrap_err();
    assert!(!format!("{error:#}").contains("private-record-marker"));
}

impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let core = LedgerStore::connect(&format!(
            "sqlite://{}",
            root.path().join("core.db").display()
        ))
        .await
        .unwrap();
        core.initialize_reward_funding().await.unwrap();
        let mut policy = super::super::tests::policy(root.path());
        policy.epoch = core.overlay_stream_id().await.unwrap();
        let url = format!("sqlite://{}", root.path().join("membership.db").display());
        let member = Store::connect(&url).await.unwrap();
        member
            .initialize(
                &policy.owner,
                &Settings {
                    minimum_sats: 10,
                    max_members: 10,
                    member_percent: 10,
                },
                0,
            )
            .await
            .unwrap();
        member
            .initialize_core_ingress(
                &policy.owner,
                &CoreBinding {
                    core_uid: policy.components[&Role::Core].uid,
                    epoch: policy.epoch,
                    start_after: 0,
                    credit_pool: "herd".into(),
                    payees: BTreeMap::from([("herd".into(), policy.owner.clone())]),
                },
            )
            .await
            .unwrap();
        let member_db = SqlitePool::connect(&url).await.unwrap();
        let manifest = Manifest {
            version: 1,
            id: Uuid::new_v4(),
            created_at: 100,
            policy: policy.clone(),
            source_policy_sha256: "11".repeat(32),
            recovery_required: true,
            images: vec![],
            report: super::super::Report {
                engine: "3.54.1".into(),
                rows: BTreeMap::new(),
                pending_feeds: 0,
                unresolved_provider_payments: 0,
                execution_claims: 0,
                payout_held_sats: 0,
                retention: "retain_all_financial_replay_and_signed_history".into(),
                resume_authorized: false,
            },
        };
        Self {
            root,
            core,
            member,
            member_db,
            policy,
            manifest,
            after: 0,
        }
    }

    async fn feeding(&mut self, percent: u8, funded: bool) -> FeedingBarrier {
        if funded {
            self.core
                .record_payment(&SettledPayment {
                    source: "strike".into(),
                    source_id: Uuid::new_v4().to_string(),
                    payment_hash: None,
                    address_user: "herd".into(),
                    credit_pool: "herd".into(),
                    amount_msat: 1_000_000,
                    settled_at: None,
                    context_json: None,
                })
                .await
                .unwrap();
        } else {
            self.core
                .grant_operator_credit(
                    &OperatorCreditGrant {
                        id: Uuid::new_v4().to_string(),
                        amount_sats: 1000,
                        address_user: "herd".into(),
                        reason: "Synthetic review fixture".into(),
                    },
                    false,
                )
                .await
                .unwrap();
        }
        let id = self.core.begin_feed_attempt(1000).await.unwrap().unwrap();
        self.core.confirm_feed_attempt(id).await.unwrap();
        let batch = self
            .core
            .core_event_batch(Some(self.policy.epoch), self.after, 128)
            .await
            .unwrap();
        self.after = batch.through;
        let feeding = batch
            .events
            .into_iter()
            .find_map(|event| match event.body {
                EventBody::Feeding(f) if f.request_id == id => Some(f),
                _ => None,
            })
            .unwrap();
        let mut snapshot: Snapshot = self.member.snapshot(&self.policy.owner).await.unwrap();
        snapshot.settings.member_percent = percent;
        snapshot.roster.members.insert(
            key(2),
            Member {
                profile: Profile {
                    name: "Synthetic member".into(),
                    picture: None,
                    lud16: Some("member@example.com".into()),
                },
                active: true,
                amount_sats: 1,
                lifetime_sats: 1,
            },
        );
        let raw = serde_json::to_string(&feeding).unwrap();
        // Explicit synthetic immutable-record fixtures, not runtime authority.
        let mut tx = self.member_db.begin().await.unwrap();
        let seq = sqlx::query("INSERT INTO ingress(owner,source_key,kind,payload_json,state) VALUES(?,?,'feeding',?,'done')")
            .bind(&self.policy.owner).bind(format!("feeding:{id}")).bind(&raw).execute(&mut *tx).await.unwrap().last_insert_rowid();
        sqlx::query("INSERT INTO feeding_barriers(owner,request_id,ingress_seq,feeding_json,snapshot_json,conversations_json) VALUES(?,?,?,?,?,'{}')")
            .bind(&self.policy.owner).bind(id.to_string()).bind(seq).bind(&raw).bind(serde_json::to_string(&snapshot).unwrap()).execute(&mut *tx).await.unwrap();
        sqlx::query("UPDATE core_streams SET received_through=? WHERE owner=?")
            .bind(i64::try_from(self.after).unwrap())
            .bind(&self.policy.owner)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        FeedingBarrier {
            ingress_seq: seq,
            feeding,
            snapshot,
            conversations: BTreeMap::new(),
        }
    }

    async fn plan(&self, barrier: &FeedingBarrier, stage: Stage, lost_ack: bool) {
        let funding = barrier.feeding.funding.as_ref().unwrap();
        let no_payout = stage == Stage::NoPayout;
        let root_policy = RewardPolicy {
            owner: self.policy.owner.clone(),
            member_percent_ceiling: if no_payout { 0 } else { 10 },
            max_sats_per_feed: 100,
            max_sats_per_24h: 10_000,
        };
        let allowance = self
            .core
            .reward_allowance(&root_policy, self.policy.epoch, barrier.feeding.request_id)
            .await
            .unwrap();
        let recipient = crate::payout::recipient::Recipient::parse("member@example.com", br#"{"tag":"payRequest","callback":"https://pay.example.com/member","minSendable":1000,"maxSendable":1000000,"metadata":"[[\"text/plain\",\"Synthetic treats\"]]"}"#).unwrap();
        let plan = Plan {
            version: 1,
            owner: self.policy.owner.clone(),
            epoch: self.policy.epoch,
            core_uid: self.policy.components[&Role::Core].uid,
            request_id: barrier.feeding.request_id,
            barrier_hash: hash(barrier).unwrap(),
            cycle: barrier.snapshot.cycle,
            roster_revision: barrier.snapshot.revision,
            funding_hash: Some(funding.proof_hash.clone()),
            real_sats: funding.strike_sats,
            threshold_sats: 1000,
            member_percent: if no_payout { 0 } else { 10 },
            budget_sats: if no_payout { 0 } else { 100 },
            reserved_sats: if no_payout { 0 } else { 100 },
            operations_sats: if no_payout { 1000 } else { 900 },
            root_policy_hash: Some(allowance.policy_hash),
            created_at: barrier.feeding.confirmed_at,
            planning_policy: PlanningPolicy {
                fee_basis_points: 0,
                minimum_fee_sats: 1,
                minimum_net_sats: 1,
                maximum_net_sats: 100,
                profile_max_age_seconds: 3600,
                operation_addresses: vec![],
            },
            rows: if no_payout {
                vec![]
            } else {
                vec![Row {
                    pubkey: key(2),
                    profile_event_id: Some("55".repeat(32)),
                    profile: Some(barrier.snapshot.roster.members[&key(2)].profile.clone()),
                    amount_sats: 1,
                    awards: SocialAwards::default(),
                    point_units: "10".into(),
                    share_micro_percent: 10_000_000,
                    gross_sats: 100,
                    fee_limit_sats: 1,
                    net_sats: 99,
                    gross_micro_percent_of_feed: 10_000_000,
                    recipient: Some(recipient),
                    skip: None,
                    conversation: None,
                    score_basis: None,
                    zap_content: None,
                    zap_wording: None,
                }]
            },
        };
        let hash = plan.hash().unwrap();
        let reservation = if stage == Stage::Reserved || lost_ack {
            Some(
                self.core
                    .reserve_reward(
                        &root_policy,
                        self.policy.epoch,
                        barrier.feeding.request_id,
                        &hash,
                        100,
                    )
                    .await
                    .unwrap(),
            )
        } else {
            None
        };
        sqlx::query("INSERT INTO reward_plans(owner,request_id,plan_hash,plan_json,state,reservation_json) VALUES(?,?,?,?,?,?)")
            .bind(&self.policy.owner).bind(barrier.feeding.request_id.to_string()).bind(hash).bind(serde_json::to_string(&plan).unwrap())
            .bind(match stage { Stage::NoPayout => "no_payout", Stage::Reserved => "reserved", _ => "awaiting_reservation" })
            .bind(if stage == Stage::Reserved { Some(serde_json::to_string(&reservation.unwrap()).unwrap()) } else { None })
            .execute(&self.member_db).await.unwrap();
    }

    async fn review(&self) -> Result<RewardReview> {
        inspect(
            self.root.path(),
            &self.policy,
            &self.manifest,
            "aa".repeat(32),
        )
        .await
    }
}

#[tokio::test]
async fn native_backlog_covers_frozen_allocations_closed_plans_lost_acks_pending_input_and_cursor_lag()
 {
    let mut f = Fixture::new().await;
    f.feeding(0, true).await;
    f.feeding(10, false).await;
    f.feeding(10, true).await;
    let closed = f.feeding(10, true).await;
    f.plan(&closed, Stage::NoPayout, false).await;
    let pending = f.feeding(10, true).await;
    f.plan(&pending, Stage::AwaitingReservation, false).await;
    let lost = f.feeding(10, true).await;
    f.plan(&lost, Stage::AwaitingReservation, true).await;
    let reserved = f.feeding(10, true).await;
    f.plan(&reserved, Stage::Reserved, false).await;
    let report = f.review().await.unwrap();
    for stage in [
        Stage::UnplannedZeroAllocation,
        Stage::UnplannedNoStrikeFunding,
        Stage::UnplannedFundedAllocation,
        Stage::NoPayout,
        Stage::Reserved,
    ] {
        assert_eq!(report.stages[&stage], 1);
    }
    assert_eq!(report.stages[&Stage::AwaitingReservation], 2);
    assert_eq!(report.unfinalized_core_reservations, 2);
    assert_eq!(report.examples.len(), 6);
    assert!(
        !report.activation_authorized
            && report.external_reconciliation_required
            && report.all_barriers_examined
    );
    assert_eq!(report.received_through, report.core_event_head);
    assert_eq!(report.unreceived_payment_events, 0);
    assert_eq!(report.unreceived_feeding_events, 0);
    // Today's settings are not the historical barriers' allocations.
    sqlx::query("UPDATE owners SET settings_json=? WHERE owner=?")
        .bind(
            serde_json::to_string(&Settings {
                minimum_sats: 10,
                max_members: 3,
                member_percent: 0,
            })
            .unwrap(),
        )
        .bind(&f.policy.owner)
        .execute(&f.member_db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO ingress(owner,source_key,kind,payload_json) VALUES(?,'pending-feeding','feeding',?)")
        .bind(&f.policy.owner).bind(serde_json::to_string(&reserved.feeding).unwrap()).execute(&f.member_db).await.unwrap();
    sqlx::query("INSERT INTO ingress(owner,source_key,kind,payload_json) VALUES(?,'pending-social','social','\"synthetic\"')")
        .bind(&f.policy.owner).execute(&f.member_db).await.unwrap();
    f.core
        .grant_operator_credit(
            &OperatorCreditGrant {
                id: "unread-core-event".into(),
                amount_sats: 1,
                address_user: "herd".into(),
                reason: "Synthetic review lag".into(),
            },
            false,
        )
        .await
        .unwrap();
    let after = f.review().await.unwrap();
    assert_eq!(after.stages, report.stages);
    assert_eq!(after.stored_member_percent, 0);
    assert_eq!(after.stored_max_members, 3);
    assert_eq!(
        (after.pending_ingress, after.pending_feeding_ingress),
        (2, 1)
    );
    assert!(after.received_through < after.core_event_head);
    assert_eq!(after.unreceived_payment_events, 0);
    assert_eq!(after.unreceived_feeding_events, 0);
}

#[tokio::test]
async fn financial_cursor_tail_is_distinct_from_information_and_is_consumed_at_the_exact_cursor() {
    let f = Fixture::new().await;
    f.core
        .append_event("weather_status", &serde_json::json!({"synthetic": true}))
        .await
        .unwrap();
    let weather = f.review().await.unwrap();
    assert!(weather.core_event_head > weather.received_through);
    assert_eq!(
        (
            weather.unreceived_payment_events,
            weather.unreceived_feeding_events
        ),
        (0, 0)
    );
    f.core
        .record_payment(&SettledPayment {
            source: "strike".into(),
            source_id: Uuid::new_v4().to_string(),
            payment_hash: None,
            address_user: "herd".into(),
            credit_pool: "herd".into(),
            amount_msat: 1_000_000,
            settled_at: None,
            context_json: None,
        })
        .await
        .unwrap();
    let payment = f.review().await.unwrap();
    assert_eq!(
        (
            payment.unreceived_payment_events,
            payment.unreceived_feeding_events
        ),
        (1, 0)
    );
    let id = f.core.begin_feed_attempt(1000).await.unwrap().unwrap();
    f.core.confirm_feed_attempt(id).await.unwrap();
    let feeding = f.review().await.unwrap();
    assert_eq!(
        (
            feeding.unreceived_payment_events,
            feeding.unreceived_feeding_events
        ),
        (1, 1)
    );
    // Synthetic cursor fixture only. No production store write or activation.
    sqlx::query("UPDATE core_streams SET received_through=? WHERE owner=?")
        .bind(i64::try_from(payment.core_event_head).unwrap())
        .bind(&f.policy.owner)
        .execute(&f.member_db)
        .await
        .unwrap();
    let after_payment = f.review().await.unwrap();
    assert_eq!(
        (
            after_payment.unreceived_payment_events,
            after_payment.unreceived_feeding_events
        ),
        (0, 1)
    );
    sqlx::query("UPDATE core_streams SET received_through=? WHERE owner=?")
        .bind(i64::try_from(feeding.core_event_head).unwrap())
        .bind(&f.policy.owner)
        .execute(&f.member_db)
        .await
        .unwrap();
    let caught_up = f.review().await.unwrap();
    assert_eq!(
        (
            caught_up.unreceived_payment_events,
            caught_up.unreceived_feeding_events
        ),
        (0, 0)
    );
    assert!(!caught_up.activation_authorized && caught_up.external_reconciliation_required);
}

#[tokio::test]
async fn example_limit_does_not_truncate_backlog_classification_or_modify_database_images() {
    let mut f = Fixture::new().await;
    for _ in 0..35 {
        f.feeding(10, true).await;
    }
    let paths = [
        "core.db",
        "core.db-wal",
        "membership.db",
        "membership.db-wal",
    ];
    let before: Vec<_> = paths
        .iter()
        .map(|name| std::fs::read(f.root.path().join(name)).ok())
        .collect();
    let report = f.review().await.unwrap();
    assert_eq!(report.stages[&Stage::UnplannedFundedAllocation], 35);
    assert_eq!((report.examples.len(), report.additional_examples), (32, 3));
    assert!(report.all_barriers_examined && !report.activation_authorized);
    for (index, name) in paths.iter().enumerate() {
        assert_eq!(std::fs::read(f.root.path().join(name)).ok(), before[index]);
    }
}

#[tokio::test]
async fn contradictory_funding_oversized_snapshots_and_plan_hashes_refuse_instead_of_hiding_records()
 {
    for fault in ["funding", "oversized", "plan"] {
        let mut f = Fixture::new().await;
        let mut barrier = f.feeding(10, true).await;
        match fault {
            "funding" => {
                sqlx::query("DROP TRIGGER immutable_feeding_barrier")
                    .execute(&f.member_db)
                    .await
                    .unwrap();
                barrier.feeding.funding.as_mut().unwrap().proof_hash = "11".repeat(32);
                let raw = serde_json::to_string(&barrier.feeding).unwrap();
                sqlx::query("UPDATE feeding_barriers SET feeding_json=?")
                    .bind(&raw)
                    .execute(&f.member_db)
                    .await
                    .unwrap();
                sqlx::query("DROP TRIGGER immutable_ingress")
                    .execute(&f.member_db)
                    .await
                    .unwrap();
                sqlx::query("UPDATE ingress SET payload_json=?")
                    .bind(raw)
                    .execute(&f.member_db)
                    .await
                    .unwrap();
            }
            "oversized" => {
                sqlx::query("DROP TRIGGER immutable_feeding_barrier")
                    .execute(&f.member_db)
                    .await
                    .unwrap();
                sqlx::query("UPDATE feeding_barriers SET snapshot_json=?")
                    .bind(" ".repeat(JSON_BOUND + 1))
                    .execute(&f.member_db)
                    .await
                    .unwrap();
            }
            _ => {
                f.plan(&barrier, Stage::NoPayout, false).await;
                sqlx::query("DROP TRIGGER immutable_reward_plan")
                    .execute(&f.member_db)
                    .await
                    .unwrap();
                sqlx::query("UPDATE reward_plans SET plan_hash=?")
                    .bind("22".repeat(32))
                    .execute(&f.member_db)
                    .await
                    .unwrap();
            }
        }
        assert!(f.review().await.is_err(), "{fault}");
    }
}
