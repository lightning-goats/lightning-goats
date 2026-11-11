//! Synthetic core grants and authoritative-provider reads, never live money.
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
};
use serde_json::json;
use std::{collections::BTreeSet, os::unix::fs::PermissionsExt, sync::Arc};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, UnixListener},
};
use uuid::Uuid;

use super::*;
use crate::{
    config::RuntimeMode,
    core_bridge::{
        BridgeConfig, Response,
        unix::{Client, Server, self_credentials},
    },
    domain::payment::SettledPayment,
    ledger::{LedgerStore, OperatorCreditGrant, StoredStrikeReceiveRequest},
    reward_funding::{FundingPage, RewardPolicy},
};

const KEY: &str = "synthetic-payout-read-key";

#[path = "provider_workflow_tests.rs"]
mod provider_workflow_tests;

fn sample_plan(
    epoch: Uuid,
    feed: Uuid,
    a: &crate::reward_funding::Allowance,
) -> crate::cyberherd::rewards::Plan {
    use crate::cyberherd::{
        membership::Profile,
        rewards::{Plan, PlanningPolicy, Row},
        shares::SocialAwards,
    };
    let key = crate::nostr_event::test_event(2, 0, 100, "{}", vec![]).pubkey;
    let address = "member@example.com";
    Plan{version:1,owner:owner(),epoch,core_uid:self_credentials().unwrap().0,request_id:feed,
        barrier_hash:"11".repeat(32),cycle:1,roster_revision:1,funding_hash:Some(a.funding.proof_hash.clone()),
        real_sats:20,threshold_sats:23,member_percent:10,budget_sats:2,reserved_sats:2,operations_sats:21,
        root_policy_hash:Some(a.policy_hash.clone()),created_at:crate::cyberherd::calendar::now().unwrap(),
        planning_policy:PlanningPolicy{fee_basis_points:0,minimum_fee_sats:1,minimum_net_sats:1,maximum_net_sats:100,profile_max_age_seconds:300,operation_addresses:vec![]},
        rows:vec![Row{pubkey:key,profile_event_id:Some("55".repeat(32)),profile:Some(Profile{name:"Member".into(),picture:None,lud16:Some(address.into())}),
            amount_sats:1,awards:SocialAwards::default(),point_units:"10".into(),share_micro_percent:10_000_000,
            gross_sats:2,fee_limit_sats:1,net_sats:1,gross_micro_percent_of_feed:8_695_652,
            conversation:None, score_basis:None, zap_content:None,
            recipient:Some(crate::payout::recipient::Recipient::parse(address,&serde_json::to_vec(&json!({
                "tag":"payRequest","minSendable":1000,"maxSendable":100000,"callback":"https://pay.example.com/invoice",
                "metadata":"[[\"text/plain\",\"Treats\"]]"})).unwrap()).unwrap()),skip:None}]}
}

async fn qualified(f: &Fixture) -> QualifiedFunding {
    let (v, task) = verifier(f, None, "0.00000001").await;
    let mut page = authenticate(f.page(0, false).await).await;
    let mut session = Session::new(f.expected(), &page).unwrap();
    loop {
        session.accept(&page, &v).await.unwrap();
        if session.is_complete() {
            break;
        }
        page = authenticate(f.page(session.after(), false).await).await;
    }
    task.abort();
    session.finish().unwrap()
}

fn journal_policy(q: &QualifiedFunding) -> crate::payout::store::Policy {
    use crate::payout::store::{Policy, Scope};
    Policy {
        scope: Scope {
            core_uid: q.core_uid(),
            epoch: q.reservation().epoch,
            owner: q.reservation().owner.clone(),
            provider: q.provider().clone(),
        },
        member_percent_ceiling: 10,
        max_sats_per_feed: 100,
        max_sats_per_24h: 1000,
        max_fee_sats: 1,
        max_net_sats: 100,
        max_source_parts: 100,
        operation_addresses: vec![],
        receipt_relays: vec!["wss://relay.damus.io/".into()],
        execution_enabled: false,
    }
}

// Adversarial core/restore evidence, constructible here only because this is a
// private child test module. Production callers cannot create QualifiedFunding.
fn copy_evidence(q: &QualifiedFunding, plan: &crate::cyberherd::rewards::Plan) -> QualifiedFunding {
    let mut reservation = q.reservation().clone();
    reservation.request_id = plan.request_id;
    reservation.plan_hash = plan.hash().unwrap();
    QualifiedFunding {
        core_uid: q.core_uid(),
        funding: q.funding().clone(),
        threshold_sats: q.threshold_sats(),
        reservation,
        provider: q.provider().clone(),
        sources: q.sources().to_vec(),
    }
}

#[tokio::test]
async fn payout_recovery_refuses_missing_empty_old_or_rebound_journals_without_initializing_them() {
    use crate::payout::store::Store;
    let f = Fixture::new().await;
    let q = qualified(&f).await;
    let policy = journal_policy(&q);
    let private = f.root.path().join("recovery-private");
    std::fs::create_dir(&private).unwrap();
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = private.join("recovery.db");
    let url = format!("sqlite://{}", path.display());
    assert!(
        Store::connect_existing_recovery(&url, policy.clone())
            .await
            .is_err()
    );
    assert!(!path.exists());
    let db = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true),
    )
    .await
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(
        Store::connect_existing_recovery(&url, policy.clone())
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sqlite_master WHERE type='table'")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
    db.close().await;
    let _store = Store::connect(&url, policy.clone()).await.unwrap();
    if self_credentials().unwrap().0 == 0 {
        assert!(
            Store::connect_existing_recovery(&url, policy)
                .await
                .is_err()
        );
        return;
    }
    Store::connect_existing_recovery(&url, policy.clone())
        .await
        .unwrap();
    let mut changed = policy.clone();
    changed.scope.provider.account_id = Uuid::new_v4();
    assert!(
        Store::connect_existing_recovery(&url, changed)
            .await
            .is_err()
    );
    let db = sqlx::SqlitePool::connect(&url).await.unwrap();
    let tables: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type='table'")
        .fetch_one(&db)
        .await
        .unwrap();
    sqlx::query(
        "DELETE FROM _sqlx_migrations WHERE version=(SELECT MAX(version) FROM _sqlx_migrations)",
    )
    .execute(&db)
    .await
    .unwrap();
    assert!(
        Store::connect_existing_recovery(&url, policy)
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sqlite_master WHERE type='table'")
            .fetch_one(&db)
            .await
            .unwrap(),
        tables
    );
}

#[tokio::test]
async fn payout_journal_commits_once_across_concurrency_reopen_and_policy_reduction() {
    use crate::payout::store::Store;
    let f = Fixture::new().await;
    let q = qualified(&f).await;
    let url = format!("sqlite://{}", f.root.path().join("payout.db").display());
    let policy = journal_policy(&q);
    let store = Store::connect(&url, policy.clone()).await.unwrap();
    let now = f.reservation.created_at + 10;
    let (one, two) = tokio::join!(store.admit(&f.plan, &q, now), store.admit(&f.plan, &q, now));
    let grant = one.unwrap();
    assert_eq!(two.unwrap(), grant);
    assert_eq!(store.held_sats().await.unwrap(), 2);
    let replay = Store::connect(&url, policy.clone()).await.unwrap();
    assert_eq!(replay.admit(&f.plan, &q, now + 86401).await.unwrap(), grant);
    let db = sqlx::SqlitePool::connect(&url).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM payout_journal")
            .fetch_one(&db)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT SUM(consumed_sats) FROM payout_sources")
            .fetch_one(&db)
            .await
            .unwrap(),
        20
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA synchronous")
            .fetch_one(&db)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
            .fetch_one(&db)
            .await
            .unwrap(),
        "wal"
    );
    let mut reduced = policy.clone();
    reduced.max_sats_per_24h = 1;
    let held = Store::connect(&url, reduced).await.unwrap();
    assert_eq!(held.grant(grant.id).await.unwrap(), Some(grant.clone()));
    assert_eq!(held.held_sats().await.unwrap(), 2);
    for table in [
        "payout_binding",
        "payout_grants",
        "payout_sources",
        "payout_source_parts",
        "payout_rows",
        "payout_journal",
        "payout_clock",
    ] {
        assert!(
            sqlx::query(&format!("DELETE FROM {table}"))
                .execute(&db)
                .await
                .is_err(),
            "{table}"
        );
    }
    assert!(
        sqlx::query("UPDATE payout_rows SET net_sats=2")
            .execute(&db)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE payout_sources SET consumed_sats=consumed_sats-1")
            .execute(&db)
            .await
            .is_err()
    );
    for scope in [
        {
            let mut p = policy.clone();
            p.scope.epoch = Uuid::new_v4();
            p
        },
        {
            let mut p = policy.clone();
            p.scope.provider.account_id = Uuid::new_v4();
            p
        },
        {
            let mut p = policy.clone();
            p.scope.provider.credential_fingerprint = "33".repeat(32);
            p
        },
        {
            let mut p = policy.clone();
            p.scope.core_uid += 1;
            p
        },
    ] {
        assert!(Store::connect(&url, scope).await.is_err());
    }
}

#[tokio::test]
async fn payout_journal_rejects_restore_reconsumption_and_rolls_back_every_partial_write() {
    use crate::payout::store::Store;
    let f = Fixture::new().await;
    let q = qualified(&f).await;
    let url = format!("sqlite://{}", f.root.path().join("payout.db").display());
    let store = Store::connect(&url, journal_policy(&q)).await.unwrap();
    let now = f.reservation.created_at + 10;
    store.admit(&f.plan, &q, now).await.unwrap();
    let mut plan = f.plan.clone();
    plan.request_id = Uuid::new_v4();
    let mut attack = copy_evidence(&q, &plan);
    attack.reservation.id = Uuid::new_v4();
    for s in &mut attack.sources {
        s.ledger_id += 1000;
    }
    // Changing local ledger IDs does not make a spent provider receive new.
    assert!(store.admit(&plan, &attack, now + 1).await.is_err());
    assert!(store.grant(attack.reservation.id).await.unwrap().is_none());
    assert_eq!(store.held_sats().await.unwrap(), 2);
    let db = sqlx::SqlitePool::connect(&url).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM payout_journal")
            .fetch_one(&db)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT SUM(consumed_sats) FROM payout_sources")
            .fetch_one(&db)
            .await
            .unwrap(),
        20
    );
    attack.sources[0].receive_request_id = Uuid::new_v4();
    assert!(store.admit(&plan, &attack, now + 1).await.is_err());
    assert_eq!(store.held_sats().await.unwrap(), 2);
}

#[tokio::test]
async fn payout_journal_accepts_disjoint_parts_of_one_verified_receive_across_feedings() {
    use crate::payout::store::Store;
    let mut f = Fixture::new().await;
    let receive = Uuid::new_v4();
    f.ids.clear();
    f.ids.insert(receive);
    f.ledger
        .record_payment(&SettledPayment {
            source: "strike".into(),
            source_id: receive.to_string(),
            payment_hash: None,
            address_user: "herd".into(),
            credit_pool: "herd".into(),
            amount_msat: 100_000,
            settled_at: None,
            context_json: Some(
                json!({"receive_request_id":f.request.receive_request_id,"receive_type":"P2P"})
                    .to_string(),
            ),
        })
        .await
        .unwrap();
    let url = format!("sqlite://{}", f.root.path().join("payout.db").display());
    let mut store = None;
    for step in 0..2 {
        let feed = f.ledger.begin_feed_attempt(10).await.unwrap().unwrap();
        f.ledger.confirm_feed_attempt(feed).await.unwrap();
        let allowance = f
            .ledger
            .reward_allowance(&f.policy, f.reservation.epoch, feed)
            .await
            .unwrap();
        let mut plan = sample_plan(f.reservation.epoch, feed, &allowance);
        plan.real_sats = 10;
        plan.threshold_sats = 10;
        plan.budget_sats = 1;
        plan.reserved_sats = 1;
        plan.operations_sats = 9;
        plan.rows[0].gross_sats = 1;
        plan.rows[0].fee_limit_sats = 0;
        plan.rows[0].gross_micro_percent_of_feed = 10_000_000;
        plan.planning_policy.minimum_fee_sats = 0;
        f.reservation = f
            .ledger
            .reserve_reward(
                &f.policy,
                f.reservation.epoch,
                feed,
                &plan.hash().unwrap(),
                1,
            )
            .await
            .unwrap();
        let (v, task) = verifier(&f, None, "0.00000100").await;
        let page = authenticate(f.page(0, false).await).await;
        let mut session = Session::new(f.expected(), &page).unwrap();
        session.accept(&page, &v).await.unwrap();
        let q = session.finish().unwrap();
        assert_eq!(q.sources()[0].receive_id, receive);
        assert_eq!(
            (q.sources()[0].received_sats, q.sources()[0].consumed_sats),
            (100, 10)
        );
        if store.is_none() {
            store = Some(Store::connect(&url, journal_policy(&q)).await.unwrap());
        }
        let now = f.reservation.created_at + 10 + step * 100;
        let grant = store.as_ref().unwrap().admit(&plan, &q, now).await.unwrap();
        let store = store.as_ref().unwrap();
        let intent = store
            .prepare_invoice(grant.id, &plan.rows[0].pubkey, now)
            .await
            .unwrap();
        let request = store
            .freeze_request(intent.id(), None, now + 1)
            .await
            .unwrap();
        let encoded = crate::test_invoices::invoice_at(
            1000,
            request.description_hash(),
            std::time::Duration::from_secs((now + 2).try_into().unwrap()),
        );
        let invoice = request
            .validate_response(
                &serde_json::to_vec(&json!({"pr":encoded})).unwrap(),
                now + 2,
            )
            .unwrap();
        if step == 0 {
            store.freeze_invoice(&invoice, now + 3).await.unwrap();
        } else {
            // Distinct funding/grants must not pay one BOLT11 payment hash twice.
            assert!(store.freeze_invoice(&invoice, now + 3).await.is_err());
            assert!(store.saved_invoice(intent.id()).await.unwrap().is_none());
            assert!(store.invoice_request(intent.id()).await.is_ok());
        }
        task.abort();
    }
    let db = sqlx::SqlitePool::connect(&url).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT consumed_sats FROM payout_sources")
            .fetch_one(&db)
            .await
            .unwrap(),
        20
    );
    assert_eq!(store.unwrap().held_sats().await.unwrap(), 2);
    assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 80);
}

#[tokio::test]
async fn outgoing_ordinary_invoice_journal_recovers_without_resigning_or_replacing_identity() {
    use crate::payout::store::Store;
    let f = Fixture::new().await;
    let q = qualified(&f).await;
    let url = format!("sqlite://{}", f.root.path().join("payout.db").display());
    let policy = journal_policy(&q);
    let store = Store::connect(&url, policy.clone()).await.unwrap();
    let now = f.reservation.created_at + 10;
    let member = &f.plan.rows[0].pubkey;
    assert!(
        store
            .prepare_invoice(q.reservation().id, member, now)
            .await
            .is_err()
    );
    let grant = store.admit(&f.plan, &q, now).await.unwrap();
    let (a, b) = tokio::join!(
        store.prepare_invoice(grant.id, member, now),
        store.prepare_invoice(grant.id, member, now)
    );
    let intent = a.unwrap();
    assert_eq!(b.unwrap().id(), intent.id());
    assert!(!intent.uses_nostr());
    assert!(intent.unsigned_zap().unwrap().is_none());
    assert!(store.invoice_request(intent.id()).await.is_err());
    let request = store
        .freeze_request(intent.id(), None, now + 1)
        .await
        .unwrap();
    let restored = Store::connect(&url, policy).await.unwrap();
    assert_eq!(
        restored
            .prepare_invoice(grant.id, member, now + 1000)
            .await
            .unwrap()
            .id(),
        intent.id()
    );
    assert!(
        restored
            .invoice_request(intent.id())
            .await
            .unwrap()
            .raw_zap()
            .is_none()
    );
    let encoded = crate::test_invoices::invoice_at(
        1000,
        request.description_hash(),
        std::time::Duration::from_secs((now + 2).try_into().unwrap()),
    );
    let invoice = request
        .validate_response(
            &serde_json::to_vec(&json!({"pr":encoded})).unwrap(),
            now + 2,
        )
        .unwrap();
    restored.freeze_invoice(&invoice, now + 3).await.unwrap();
    assert_eq!(
        restored.freeze_invoice(&invoice, now + 1000).await.unwrap(),
        invoice
    );
    assert_eq!(
        restored.saved_invoice(intent.id()).await.unwrap(),
        Some(invoice.clone())
    );
    assert!(restored.invoice_request(intent.id()).await.is_err());
    let changed = crate::test_invoices::invoice_at(
        1000,
        request.description_hash(),
        std::time::Duration::from_secs((now + 3).try_into().unwrap()),
    );
    let changed = request
        .validate_response(
            &serde_json::to_vec(&json!({"pr":changed})).unwrap(),
            now + 3,
        )
        .unwrap();
    assert!(restored.freeze_invoice(&changed, now + 4).await.is_err());
    assert_eq!(
        restored.saved_invoice(intent.id()).await.unwrap(),
        Some(invoice)
    );
    assert_eq!(restored.held_sats().await.unwrap(), 2);
    let db = sqlx::SqlitePool::connect(&url).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM payout_invoice_journal")
            .fetch_one(&db)
            .await
            .unwrap(),
        3
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT protocol FROM payout_callback_requests")
            .fetch_one(&db)
            .await
            .unwrap(),
        "ordinary"
    );
    for table in [
        "payout_invoice_intents",
        "payout_callback_requests",
        "payout_invoices",
        "payout_invoice_journal",
    ] {
        assert!(
            sqlx::query(&format!("DELETE FROM {table}"))
                .execute(&db)
                .await
                .is_err()
        );
    }
    assert!(
        sqlx::query("UPDATE payout_callback_requests SET raw_zap='{}',protocol='nip57'")
            .execute(&db)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn outgoing_supported_zap_freezes_exact_bytes_before_callback_and_rejects_downgrades() {
    use crate::payout::store::Store;
    let f = Fixture::new_with_nostr(true).await;
    let q = qualified(&f).await;
    let url = format!("sqlite://{}", f.root.path().join("payout.db").display());
    let policy = journal_policy(&q);
    let store = Store::connect(&url, policy.clone()).await.unwrap();
    let now = f.reservation.created_at + 10;
    let grant = store.admit(&f.plan, &q, now).await.unwrap();
    let intent = store
        .prepare_invoice(grant.id, &f.plan.rows[0].pubkey, now)
        .await
        .unwrap();
    let unsigned = intent.unsigned_zap().unwrap().unwrap();
    assert_eq!(unsigned.kind, 9734);
    assert!(store.freeze_request(intent.id(), None, now).await.is_err());
    let event = crate::nostr_event::test_event(
        1,
        unsigned.kind,
        unsigned.created_at,
        &unsigned.content,
        unsigned.tags,
    );
    let pretty = serde_json::to_string_pretty(&event).unwrap();
    let request = store
        .freeze_request(intent.id(), Some(&pretty), now + 1)
        .await
        .unwrap();
    assert_eq!(
        request.description_hash(),
        hex::encode(Sha256::digest(pretty.as_bytes()))
    );
    let compact = serde_json::to_string(&event).unwrap();
    assert!(
        store
            .freeze_request(intent.id(), Some(&compact), now + 2)
            .await
            .is_err()
    );
    let restored = Store::connect(&url, policy).await.unwrap();
    let request = restored.invoice_request(intent.id()).await.unwrap();
    assert_eq!(request.raw_zap(), Some(pretty.as_str()));
    let wrong = crate::test_invoices::invoice_at(
        1000,
        &f.plan.rows[0].recipient.as_ref().unwrap().metadata_hash,
        std::time::Duration::from_secs((now + 2).try_into().unwrap()),
    );
    assert!(
        request
            .validate_response(&serde_json::to_vec(&json!({"pr":wrong})).unwrap(), now + 2)
            .is_err()
    );
    let encoded = crate::test_invoices::invoice_at(
        1000,
        request.description_hash(),
        std::time::Duration::from_secs((now + 2).try_into().unwrap()),
    );
    let invoice = request
        .validate_response(
            &serde_json::to_vec(&json!({"pr":encoded})).unwrap(),
            now + 2,
        )
        .unwrap();
    restored.freeze_invoice(&invoice, now + 3).await.unwrap();
    assert_eq!(restored.held_sats().await.unwrap(), 2);
    let db = sqlx::SqlitePool::connect(&url).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT protocol FROM payout_callback_requests")
            .fetch_one(&db)
            .await
            .unwrap(),
        "nip57"
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT raw_zap FROM payout_callback_requests")
            .fetch_one(&db)
            .await
            .unwrap(),
        pretty
    );
}

#[tokio::test]
async fn payout_journal_never_ages_unknown_capacity_and_rejects_clock_reversal() {
    use crate::payout::store::Store;
    let f = Fixture::new().await;
    let q = qualified(&f).await;
    let url = format!("sqlite://{}", f.root.path().join("payout.db").display());
    let mut policy = journal_policy(&q);
    policy.max_sats_per_24h = 2;
    let store = Store::connect(&url, policy.clone()).await.unwrap();
    let now = f.reservation.created_at + 10;
    store.admit(&f.plan, &q, now).await.unwrap();
    let mut plan = f.plan.clone();
    plan.request_id = Uuid::new_v4();
    let mut next = copy_evidence(&q, &plan);
    next.reservation.id = Uuid::new_v4();
    for s in &mut next.sources {
        s.receive_id = Uuid::new_v4();
        s.ledger_id += 1000;
    }
    assert!(store.admit(&plan, &next, now + 86401).await.is_err());
    let broad = Store::connect(&url, journal_policy(&q)).await.unwrap();
    assert!(broad.admit(&plan, &next, now - 1).await.is_err());
    assert!(broad.grant(next.reservation.id).await.unwrap().is_none());
    assert!(broad.admit(&plan, &next, now + 1).await.is_ok());
    assert_eq!(broad.held_sats().await.unwrap(), 4);
}

#[tokio::test]
async fn payout_journal_revalidates_root_amount_and_destination_limits_and_rejects_other_databases()
{
    use crate::payout::store::Store;
    let f = Fixture::new().await;
    let q = qualified(&f).await;
    let url = format!("sqlite://{}", f.root.path().join("payout.db").display());
    let policy = journal_policy(&q);
    let store = Store::connect(&url, policy.clone()).await.unwrap();
    let now = f.reservation.created_at + 10;
    let mut plans = vec![];
    let mut p = f.plan.clone();
    p.rows[0].pubkey = owner();
    plans.push(p);
    let mut p = f.plan.clone();
    p.rows[0].net_sats = 2;
    plans.push(p);
    let mut p = f.plan.clone();
    p.member_percent = 11;
    plans.push(p);
    let mut p = f.plan.clone();
    p.operations_sats = 0;
    plans.push(p);
    let mut p = f.plan.clone();
    p.rows[0].recipient.as_mut().unwrap().callback = "https://127.0.0.1/pay".into();
    plans.push(p);
    let mut p = f.plan.clone();
    p.rows.push(p.rows[0].clone());
    plans.push(p);
    for p in plans {
        let evidence = copy_evidence(&q, &p);
        assert!(store.admit(&p, &evidence, now).await.is_err());
    }
    let mut ops = policy.clone();
    ops.operation_addresses = vec!["MEMBER@example.com".into()];
    let ops = Store::connect(&url, ops).await.unwrap();
    assert!(ops.admit(&f.plan, &q, now).await.is_err());
    assert_eq!(store.held_sats().await.unwrap(), 0);
    assert!(Store::connect(&f.database, policy).await.is_err());
    let db = sqlx::SqlitePool::connect(&f.database).await.unwrap();
    let tables: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE name='payout_grants'")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(tables, 0);
}
fn owner() -> String {
    crate::nostr_event::test_event(1, 1, 100, "", vec![]).pubkey
}

struct Fixture {
    root: TempDir,
    database: String,
    ledger: LedgerStore,
    policy: RewardPolicy,
    reservation: Reservation,
    request: StoredStrikeReceiveRequest,
    plan: crate::cyberherd::rewards::Plan,
    ids: BTreeSet<Uuid>,
}
impl Fixture {
    async fn new() -> Self {
        Self::new_with_nostr(false).await
    }
    async fn new_with_nostr(nostr: bool) -> Self {
        let root = TempDir::new().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o750)).unwrap();
        let database = format!("sqlite://{}", root.path().join("core.db").display());
        let ledger = LedgerStore::connect(&database).await.unwrap();
        let epoch = ledger.overlay_stream_id().await.unwrap();
        ledger.initialize_reward_funding().await.unwrap();
        let request = StoredStrikeReceiveRequest {
            receive_request_id: Uuid::new_v4(),
            address_user: "herd".into(),
            credit_pool: "herd".into(),
            amount_msat: 1_000_000,
            description_hash: "11".repeat(32),
            payment_hash: "22".repeat(32),
            invoice: crate::test_invoices::invoice(1_000_000, &"11".repeat(32)),
            created_provider: None,
        };
        ledger
            .record_strike_receive_request(&request)
            .await
            .unwrap();
        ledger
            .grant_operator_credit(
                &OperatorCreditGrant {
                    id: "virtual".into(),
                    amount_sats: 3,
                    address_user: "herd".into(),
                    reason: "Synthetic fixture".into(),
                },
                false,
            )
            .await
            .unwrap();
        let mut ids = BTreeSet::new();
        for _ in 0..20 {
            let id = Uuid::new_v4();
            ids.insert(id);
            ledger.record_payment(&SettledPayment{source:"strike".into(),source_id:id.to_string(),payment_hash:None,
                address_user:"herd".into(),credit_pool:"herd".into(),amount_msat:1000,settled_at:None,
                context_json:Some(json!({"receive_request_id":request.receive_request_id,"receive_type":"P2P"}).to_string())}).await.unwrap();
        }
        let feed = ledger.begin_feed_attempt(23).await.unwrap().unwrap();
        ledger.confirm_feed_attempt(feed).await.unwrap();
        let policy = RewardPolicy {
            owner: owner(),
            member_percent_ceiling: 10,
            max_sats_per_feed: 100,
            max_sats_per_24h: 1000,
        };
        let allowance = ledger.reward_allowance(&policy, epoch, feed).await.unwrap();
        let mut plan = sample_plan(epoch, feed, &allowance);
        if nostr {
            plan.rows[0].recipient.as_mut().unwrap().receipt_pubkey =
                Some(crate::nostr_event::test_event(3, 1, 100, "", vec![]).pubkey);
        }
        let reservation = ledger
            .reserve_reward(&policy, epoch, feed, &plan.hash().unwrap(), 2)
            .await
            .unwrap();
        Self {
            root,
            database,
            ledger,
            policy,
            reservation,
            request,
            plan,
            ids,
        }
    }
    async fn page(&self, after: i64, enabled: bool) -> FundingPage {
        self.ledger
            .payout_funding_page(
                self.reservation.epoch,
                self.reservation.id,
                &self.policy.owner,
                after,
                Some(&self.policy),
                enabled,
            )
            .await
            .unwrap()
    }
    fn expected(&self) -> ExpectedPlan {
        ExpectedPlan {
            core_uid: self_credentials().unwrap().0,
            epoch: self.reservation.epoch,
            owner: self.policy.owner.clone(),
            plan_hash: self.reservation.plan_hash.clone(),
            member_percent_ceiling: 10,
            per_feed_sats: 100,
            max_parts: 100,
        }
    }
}

async fn authenticate(page: FundingPage) -> AuthenticatedFundingPage {
    let root = TempDir::new().unwrap();
    let path = root.path().join("core.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let scope = (
        page.reservation.epoch,
        page.reservation.id,
        page.reservation.owner.clone(),
        page.after,
    );
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let len = stream.read_u32().await.unwrap();
        let mut raw = vec![0; len as usize];
        stream.read_exact(&mut raw).await.unwrap();
        let raw = serde_json::to_vec(&Response::PayoutFunding {
            page: Box::new(page),
        })
        .unwrap();
        stream.write_u32(raw.len() as u32).await.unwrap();
        stream.write_all(&raw).await.unwrap();
    });
    let client = Client::new(path, self_credentials().unwrap().0).unwrap();
    let authenticated = client
        .payout_funding(scope.0, scope.1, &scope.2, scope.3)
        .await
        .unwrap();
    task.await.unwrap();
    authenticated
}

#[derive(Clone)]
struct Provider {
    request: StoredStrikeReceiveRequest,
    ids: BTreeSet<Uuid>,
    reject: Option<Uuid>,
    amount: &'static str,
}
async fn request(
    State(s): State<Arc<Provider>>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> impl IntoResponse {
    if h.get("authorization").and_then(|s| s.to_str().ok())
        != Some("Bearer synthetic-payout-read-key")
        || id != s.request.receive_request_id
    {
        return (StatusCode::FORBIDDEN, Json(json!({"error":"denied"})));
    }
    (
        StatusCode::OK,
        Json(
            json!({"receiveRequestId":id,"targetCurrency":"BTC","bolt11":{
        "invoice":s.request.invoice,"paymentHash":s.request.payment_hash,"descriptionHash":s.request.description_hash,
        "requestedAmount":{"amount":"0.00001000","currency":"BTC"}}}),
        ),
    )
}
async fn receives(
    State(s): State<Arc<Provider>>,
    Path(id): Path<Uuid>,
    Query(q): Query<std::collections::BTreeMap<String, String>>,
    h: HeaderMap,
) -> impl IntoResponse {
    let receive = q.get("$receiveId").and_then(|v| v.parse::<Uuid>().ok());
    if h.get("authorization").and_then(|s| s.to_str().ok())
        != Some("Bearer synthetic-payout-read-key")
        || id != s.request.receive_request_id
        || receive.is_none_or(|r| !s.ids.contains(&r) || s.reject == Some(r))
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"synthetic-private-provider-body"})),
        );
    }
    (
        StatusCode::OK,
        Json(
            json!({"items":[{"receiveId":receive,"receiveRequestId":id,"type":"P2P","state":"COMPLETED",
        "amountReceived":{"amount":s.amount,"currency":"BTC"},"amountCredited":{"amount":s.amount,"currency":"BTC"}}]}),
        ),
    )
}
async fn verifier(
    f: &Fixture,
    reject: Option<Uuid>,
    amount: &'static str,
) -> (SourceVerifier, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/v1/receive-requests/{id}", get(request))
        .route("/v1/receive-requests/{id}/receives", get(receives))
        .with_state(Arc::new(Provider {
            request: f.request.clone(),
            ids: f.ids.clone(),
            reject,
            amount,
        }));
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let scope = SourceVerifier::scope(
        Uuid::from_u128(1),
        &hex::encode(Sha256::digest(KEY.as_bytes())),
        KEY,
    )
    .unwrap();
    let verifier = SourceVerifier {
        client: StrikeClient::new(&format!("http://{address}"), KEY.into()).unwrap(),
        scope,
    };
    (verifier, task)
}

#[tokio::test]
async fn complete_bounded_pages_qualify_actual_p2p_amounts_and_exclude_virtual_funds() {
    let f = Fixture::new().await;
    let (v, task) = verifier(&f, None, "0.00000001").await;
    let first = authenticate(f.page(0, true).await).await;
    assert_eq!(
        (
            first.page().threshold_sats,
            first.page().funding.strike_sats
        ),
        (23, 20)
    );
    assert_eq!(first.page().parts.len(), 8);
    assert!(first.page().parts[0].anchor.is_none());
    let mut session = Session::new(f.expected(), &first).unwrap();
    let mut page = first;
    while !session.is_complete() {
        session.accept(&page, &v).await.unwrap();
        if !session.is_complete() {
            page = authenticate(f.page(session.after(), true).await).await;
        }
    }
    let qualified = session.finish().unwrap();
    assert_eq!(qualified.core_uid(), f.expected().core_uid);
    assert_eq!(
        (qualified.funding().strike_sats, qualified.threshold_sats()),
        (20, 23)
    );
    assert_eq!(qualified.reservation(), &f.reservation);
    assert_eq!(qualified.provider(), v.provider_scope());
    assert_eq!(qualified.sources().len(), 20);
    assert_eq!(
        qualified
            .sources()
            .iter()
            .map(|r| r.consumed_sats)
            .sum::<u64>(),
        20
    );
    assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 0);
    task.abort();
}

#[tokio::test]
async fn failed_provider_read_rolls_back_page_progress_and_retry_checks_same_sources() {
    let f = Fixture::new().await;
    let first = f.page(0, true).await;
    let reject = first
        .parts
        .iter()
        .filter_map(|r| r.anchor.as_ref())
        .nth(2)
        .unwrap()
        .receive_id;
    let page = authenticate(first).await;
    let mut session = Session::new(f.expected(), &page).unwrap();
    let (v, bad) = verifier(&f, Some(reject), "0.00000001").await;
    let error = session.accept(&page, &v).await.unwrap_err();
    assert!(!format!("{error:#}").contains("synthetic-private-provider-body"));
    assert_eq!(session.after(), 0);
    assert_eq!(session.count, 0);
    assert!(session.sources.is_empty());
    bad.abort();
    let (v, good) = verifier(&f, None, "0.00000001").await;
    session.accept(&page, &v).await.unwrap();
    assert_eq!(session.after(), page.page().through);
    good.abort();
}

#[tokio::test]
async fn wrong_account_view_amount_duplicate_receive_and_incomplete_sequence_cannot_qualify() {
    let f = Fixture::new().await;
    let first = authenticate(f.page(0, true).await).await;
    let session = Session::new(f.expected(), &first).unwrap();
    assert!(session.finish().is_err());
    let (v, task) = verifier(&f, None, "0.00000002").await;
    let mut session = Session::new(f.expected(), &first).unwrap();
    assert!(session.accept(&first, &v).await.is_err());
    assert_eq!(session.after(), 0);
    task.abort();
    let (v, task) = verifier(&f, None, "0.00000001").await;
    let mut wrong = f.page(0, true).await;
    wrong.parts[3].anchor = wrong.parts[2].anchor.clone();
    let wrong = authenticate(wrong).await;
    let mut session = Session::new(f.expected(), &wrong).unwrap();
    assert!(session.accept(&wrong, &v).await.is_err());
    assert_eq!(session.after(), 0);
    let mut wrong = first.page().clone();
    wrong.parts[2].anchor.as_mut().unwrap().receive_id = Uuid::new_v4();
    let wrong = authenticate(wrong).await;
    let mut session = Session::new(f.expected(), &wrong).unwrap();
    assert!(session.accept(&wrong, &v).await.is_err());
    task.abort();
}

#[tokio::test]
async fn scope_limits_provider_changes_and_skipped_pages_are_rejected() {
    let f = Fixture::new().await;
    let first = authenticate(f.page(0, true).await).await;
    for expected in [
        ExpectedPlan {
            plan_hash: "cd".repeat(32),
            ..f.expected()
        },
        ExpectedPlan {
            core_uid: f.expected().core_uid + 1,
            ..f.expected()
        },
        ExpectedPlan {
            member_percent_ceiling: 0,
            ..f.expected()
        },
        ExpectedPlan {
            per_feed_sats: 1,
            ..f.expected()
        },
    ] {
        assert!(Session::new(expected, &first).is_err());
    }
    let (v, task) = verifier(&f, None, "0.00000001").await;
    let mut session = Session::new(
        ExpectedPlan {
            max_parts: 5,
            ..f.expected()
        },
        &first,
    )
    .unwrap();
    assert!(session.accept(&first, &v).await.is_err());
    let next = authenticate(f.page(first.page().through, true).await).await;
    assert!(Session::new(f.expected(), &next).is_err());
    let mut session = Session::new(f.expected(), &first).unwrap();
    assert!(session.accept(&next, &v).await.is_err());
    session.accept(&first, &v).await.unwrap();
    let (mut changed, other) = verifier(&f, None, "0.00000001").await;
    changed.scope.account_id = Uuid::new_v4();
    assert!(session.accept(&next, &changed).await.is_err());
    task.abort();
    other.abort();
}

#[tokio::test]
async fn core_role_reads_are_separate_from_intake_and_never_enable_execution_by_default() {
    for (payout, enabled, mode) in [
        (false, true, RuntimeMode::Active),
        (true, false, RuntimeMode::Active),
        (true, true, RuntimeMode::Shadow),
        (true, true, RuntimeMode::Canary),
        (true, true, RuntimeMode::Active),
    ] {
        let f = Fixture::new().await;
        let (uid, gid) = self_credentials().unwrap();
        let config = BridgeConfig {
            socket: f.root.path().join("bridge.sock"),
            peer_uid: if payout { uid + 1 } else { uid },
            group_gid: gid,
            reward_policy: Some(f.policy.clone()),
            payout_peer_uid: Some(if payout { uid } else { uid + 1 }),
            delegated_feeder_messages: false,
            payout_execution_enabled: enabled,
            payout_account_id: None,
        };
        let server = Server::bind(&config).await.unwrap();
        let task = tokio::spawn(server.run(f.ledger.clone(), mode));
        let client = Client::new(config.socket.clone(), uid).unwrap();
        let page = client
            .payout_funding(f.reservation.epoch, f.reservation.id, &owner(), 0)
            .await;
        if payout {
            let page = page.unwrap();
            // Legacy funding reads never authorize execution.
            assert!(!page.page().execution_enabled);
            assert_eq!(page.page().nostr_enabled, mode.nostr_enabled());
            assert!(client.events(None, 0, 1).await.is_err());
            assert!(
                client
                    .reserve_reward(
                        f.reservation.epoch,
                        f.reservation.request_id,
                        &owner(),
                        &f.reservation.plan_hash,
                        2
                    )
                    .await
                    .is_err()
            );
            assert!(
                client
                    .reward_allowance(f.reservation.epoch, f.reservation.request_id, &owner())
                    .await
                    .is_err()
            );
        } else {
            assert!(page.is_err());
            assert!(client.events(None, 0, 1).await.is_ok());
        }
        task.abort();
        let _ = task.await;
    }
}

#[tokio::test]
async fn consumed_source_changes_missing_correlations_wrong_scope_and_policy_holds_fail_closed() {
    let f = Fixture::new().await;
    let page = f.page(0, true).await;
    assert!(page.execution_permitted(page.observed_at));
    assert!(!page.execution_permitted(page.observed_at + 6));
    assert!(!page.execution_permitted(page.observed_at - 1));
    let page = f
        .ledger
        .payout_funding_page(
            f.reservation.epoch,
            f.reservation.id,
            &owner(),
            0,
            None,
            true,
        )
        .await
        .unwrap();
    assert!(!page.execution_enabled);
    let zero = RewardPolicy {
        max_sats_per_feed: 0,
        ..f.policy.clone()
    };
    assert!(
        !f.ledger
            .payout_funding_page(
                f.reservation.epoch,
                f.reservation.id,
                &owner(),
                0,
                Some(&zero),
                true
            )
            .await
            .unwrap()
            .execution_enabled
    );
    for (epoch, id, who, cursor) in [
        (Uuid::new_v4(), f.reservation.id, owner(), 0),
        (f.reservation.epoch, Uuid::new_v4(), owner(), 0),
        (
            f.reservation.epoch,
            f.reservation.id,
            crate::nostr_event::test_event(2, 1, 100, "", vec![]).pubkey,
            0,
        ),
        (f.reservation.epoch, f.reservation.id, owner(), 999999),
    ] {
        assert!(
            f.ledger
                .payout_funding_page(epoch, id, &who, cursor, Some(&f.policy), true)
                .await
                .is_err()
        );
    }
    let pool = sqlx::SqlitePool::connect(&f.database).await.unwrap();
    sqlx::query("UPDATE settled_payments SET context_json=NULL")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        f.ledger
            .payout_funding_page(
                f.reservation.epoch,
                f.reservation.id,
                &owner(),
                0,
                Some(&f.policy),
                true
            )
            .await
            .is_err()
    );
    assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 0);
}

#[test]
fn payout_identity_configuration_and_key_fingerprint_cannot_be_silently_rebound() {
    let good = BridgeConfig {
        socket: "/run/example/core.sock".into(),
        peer_uid: 10001,
        group_gid: 10002,
        reward_policy: None,
        payout_peer_uid: None,
        delegated_feeder_messages: false,
        payout_execution_enabled: false,
        payout_account_id: None,
    };
    for c in [
        BridgeConfig {
            payout_peer_uid: Some(0),
            ..good.clone()
        },
        BridgeConfig {
            payout_peer_uid: Some(10001),
            ..good.clone()
        },
        BridgeConfig {
            delegated_feeder_messages: false,
            payout_execution_enabled: true,
            payout_account_id: None,
            ..good
        },
    ] {
        assert!(c.validate().is_err());
    }
    let fingerprint = hex::encode(Sha256::digest(KEY.as_bytes()));
    assert!(SourceVerifier::new(Uuid::nil(), &fingerprint, KEY.into()).is_err());
    assert!(SourceVerifier::new(Uuid::new_v4(), &"00".repeat(32), KEY.into()).is_err());
    assert!(SourceVerifier::new(Uuid::new_v4(), &fingerprint, format!("{KEY}\n")).is_err());
    assert!(SourceVerifier::new(Uuid::new_v4(), &fingerprint, KEY.into()).is_ok());
}

#[tokio::test]
async fn inclusive_fee_observations_survive_reopen_without_mutating_grants() {
    use crate::payout::store::Store;
    let f = Fixture::new().await;
    let q = qualified(&f).await;
    let url = format!(
        "sqlite://{}",
        f.root.path().join("inclusive-fees.db").display()
    );
    let policy = journal_policy(&q);
    let store = Store::connect(&url, policy.clone()).await.unwrap();
    let grant = store
        .admit(&f.plan, &q, f.reservation.created_at + 10)
        .await
        .unwrap();
    let db = sqlx::SqlitePool::connect(&url).await.unwrap();
    let member = f.plan.rows[0].pubkey.clone();
    let insert = |round: i64, net: i64, fee: i64, gross: i64| {
        let (db, id, member) = (&db, grant.id.to_string(), member.clone());
        async move {
            sqlx::query("INSERT INTO payout_fee_observations(grant_id,member,round,provider_quote_id,gross_sats,invoice_net_sats,fee_sats,total_sats,observed_at,document_json) VALUES(?,?,?,?,?,?,?,?,?,?)")
                .bind(id).bind(member).bind(round).bind(Uuid::new_v4().to_string())
                .bind(gross).bind(net).bind(fee).bind(net + fee).bind(grant.admitted_at).bind("{}")
                .execute(db).await
        }
    };
    let probe = |round: i64, net: i64, previous: Option<String>| {
        let (db, id, member, created) =
            (&db, grant.id.to_string(), member.clone(), grant.admitted_at);
        async move {
            sqlx::query("INSERT INTO payout_fee_probe_intents(id,grant_id,member,round,previous_quote_id,net_sats,document_json,created_at) VALUES(?,?,?,?,?,?,?,?)")
                .bind(Uuid::new_v4().to_string()).bind(id).bind(member).bind(round)
                .bind(previous).bind(net).bind("{}").bind(created).execute(db).await
        }
    };
    probe(0, 2, None).await.unwrap();
    assert!(probe(0, 2, None).await.is_err());
    // The fixture's actual admitted gross is two sats. Discovery of a one-sat
    // fee requires a new one-sat invoice; the first quote cannot execute.
    assert!(insert(0, 2, 1, 3).await.is_err());
    insert(0, 2, 1, 2).await.unwrap();
    let prior: String =
        sqlx::query_scalar("SELECT provider_quote_id FROM payout_fee_observations WHERE round=0")
            .fetch_one(&db)
            .await
            .unwrap();
    assert!(probe(1, 2, Some(prior.clone())).await.is_err());
    probe(1, 1, Some(prior)).await.unwrap();
    assert!(insert(2, 1, 1, 2).await.is_err());
    assert!(insert(1, 2, 1, 2).await.is_err());
    insert(1, 1, 1, 2).await.unwrap();
    let fitted: String =
        sqlx::query_scalar("SELECT provider_quote_id FROM payout_fee_observations WHERE round=1")
            .fetch_one(&db)
            .await
            .unwrap();
    assert!(probe(2, 1, Some(fitted)).await.is_err());
    assert!(
        sqlx::query("UPDATE payout_fee_probe_intents SET net_sats=1")
            .execute(&db)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM payout_fee_probe_intents")
            .execute(&db)
            .await
            .is_err()
    );
    assert!(insert(2, 1, 0, 2).await.is_err());
    assert!(
        sqlx::query("UPDATE payout_fee_observations SET fee_sats=0")
            .execute(&db)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM payout_fee_observations")
            .execute(&db)
            .await
            .is_err()
    );
    drop(store);
    let reopened = Store::connect(&url, policy).await.unwrap();
    assert_eq!(
        reopened
            .admit(&f.plan, &q, f.reservation.created_at + 11)
            .await
            .unwrap(),
        grant
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM payout_fee_observations")
            .fetch_one(&db)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM payout_execute_intents")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
    assert_eq!(reopened.held_sats().await.unwrap(), 2);
}
