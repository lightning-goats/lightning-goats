//! Native inspection of copies of the real four-journal schemas. Provider and
//! private transport use existing synthetic fixtures; no root/service authority.
use super::*;
use crate::state_maintenance::{Component, Policy, Role, inspect_fixture};
use sqlx::{Row as _, SqlitePool};
use std::{collections::BTreeMap, path::Path};

async fn copy_database(url: &str, destination: &Path) {
    let pool = SqlitePool::connect(url).await.unwrap();
    sqlx::query("VACUUM INTO ?")
        .bind(destination.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

async fn images(root: &Path, urls: &BTreeMap<Role, String>) {
    std::fs::create_dir(root).unwrap();
    for (role, name) in [
        (Role::Core, "core.db"),
        (Role::Membership, "membership.db"),
        (Role::Payout, "payout.db"),
        (Role::Signer, "signer.db"),
    ] {
        copy_database(&urls[&role], &root.join(name)).await;
    }
}

#[tokio::test]
async fn native_four_journal_inspection_retains_original_claim_signed_bytes_failed_hold_and_cost_ack_and_rejects_payout_rollback()
 {
    let w = Workflow::new(true).await;
    let root = w.f.root.path();
    let member_url = format!("sqlite://{}", root.join("membership.db").display());
    let member = crate::cyberherd::store::Store::connect(&member_url)
        .await
        .unwrap();
    member
        .initialize(
            &w.policy.scope.owner,
            &crate::cyberherd::membership::Settings {
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
            &w.policy.scope.owner,
            &crate::cyberherd::ingress::CoreBinding {
                core_uid: w.policy.scope.core_uid,
                epoch: w.policy.scope.epoch,
                start_after: 0,
                credit_pool: "herd".into(),
                payees: [("herd".into(), w.policy.scope.owner.clone())].into(),
            },
        )
        .await
        .unwrap();
    // The existing payout fixture predates membership intake. Preserve its exact
    // frozen plan behind a synthetic ordered barrier in the real membership schema.
    let membership = SqlitePool::connect(&member_url).await.unwrap();
    let ingress = sqlx::query("INSERT INTO ingress(owner,source_key,kind,payload_json,state) VALUES(?,?,'feeding','{}','done')")
        .bind(&w.policy.scope.owner).bind(w.f.plan.request_id.to_string())
        .execute(&membership).await.unwrap().last_insert_rowid();
    sqlx::query("INSERT INTO feeding_barriers(owner,request_id,ingress_seq,feeding_json,snapshot_json) VALUES(?,?,?,'{}',?)")
        .bind(&w.policy.scope.owner).bind(w.f.plan.request_id.to_string()).bind(ingress)
        .bind(serde_json::to_string(&member.snapshot(&w.policy.scope.owner).await.unwrap()).unwrap())
        .execute(&membership).await.unwrap();
    sqlx::query("INSERT INTO reward_plans(owner,request_id,plan_hash,plan_json,state,reservation_json) VALUES(?,?,?,?,'reserved',?)")
        .bind(&w.policy.scope.owner).bind(w.f.plan.request_id.to_string())
        .bind(w.f.plan.hash().unwrap()).bind(serde_json::to_string(&w.f.plan).unwrap())
        .bind(serde_json::to_string(&w.f.reservation).unwrap())
        .execute(&membership).await.unwrap();
    membership.close().await;
    let core_uid = w.policy.scope.core_uid;
    let components: BTreeMap<_, _> = [Role::Core, Role::Membership, Role::Payout, Role::Signer]
        .into_iter()
        .enumerate()
        .map(|(index, role)| {
            (
                role,
                Component {
                    database: root.join(format!("logical-{index}.db")),
                    uid: core_uid + index as u32,
                    gid: core_uid + index as u32,
                    unit: format!("fixture-{index}.service"),
                },
            )
        })
        .collect();
    let p = Policy {
        version: 1,
        owner: w.policy.scope.owner.clone(),
        epoch: w.policy.scope.epoch,
        provider: w.policy.scope.provider.clone(),
        max_component_bytes: 128 * 1024 * 1024,
        components,
    };
    let signer_url = format!("sqlite://{}", root.join("signer.db").display());
    let _signer = crate::payout::signer::Store::connect(
        &signer_url,
        crate::payout::signer::Policy {
            owner: p.owner.clone(),
            client_pubkey: crate::nostr_event::test_event(4, 0, 100, "", vec![]).pubkey,
            peer_uid: p.components[&Role::Payout].uid,
            relays: vec!["wss://relay.damus.io/".into()],
            max_net_sats: 0,
            max_new_per_hour: 0,
        },
    )
    .await
    .unwrap();
    let payout = SqlitePool::connect(&w.url).await.unwrap();
    let intent = sqlx::query("SELECT document_json,raw_zap FROM payout_invoice_intents i JOIN payout_callback_requests r ON r.intent_id=i.id WHERE i.id=?")
        .bind(w.invoice_id.to_string()).fetch_one(&payout).await.unwrap();
    let document: String = intent.get("document_json");
    let signed: String = intent.get("raw_zap");
    let signer = SqlitePool::connect(&signer_url).await.unwrap();
    sqlx::query("INSERT INTO signer_intents(id,document_json,created_at) VALUES(?,?,?)")
        .bind(w.invoice_id.to_string())
        .bind(&document)
        .bind(now())
        .execute(&signer)
        .await
        .unwrap();
    sqlx::query("INSERT INTO signer_results(id,event_json) VALUES(?,?)")
        .bind(w.invoice_id.to_string())
        .bind(&signed)
        .execute(&signer)
        .await
        .unwrap();
    signer.close().await;
    let urls = BTreeMap::from([
        (Role::Core, w.f.database.clone()),
        (Role::Membership, member_url),
        (Role::Payout, w.url.clone()),
        (Role::Signer, signer_url),
    ]);
    let older_payout = root.join("payout-before-execution.db");
    copy_database(&w.url, &older_payout).await;
    let quote = w.quote().await;
    let permit = w
        .store
        .prepare_execute(quote.id(), &w.fresh(true, true).await, now())
        .await
        .unwrap();
    let response = w.execute(&permit).await.unwrap();
    let bound = w.store.bind_payment(&response, now()).await.unwrap();
    let pending = root.join("pending-images");
    images(&pending, &urls).await;
    let reads = w.mock.lock().unwrap().get_ids.len();
    let report = inspect_fixture(&pending, &p).await.unwrap();
    assert_eq!(report.execution_claims, 1);
    assert_eq!(report.unresolved_provider_payments, 1);
    assert_eq!(report.payout_held_sats, 2);
    assert_eq!(report.rows[&Role::Signer]["signer_results"], 1);
    assert!(!report.resume_authorized);
    assert_eq!(w.mock.lock().unwrap().get_ids.len(), reads);
    assert_eq!(w.store.held_sats().await.unwrap(), 2);
    w.mock.lock().unwrap().state = PaymentState::Failed;
    w.store
        .observe_payment(&w.sender.read_payment(&bound).await.unwrap(), now())
        .await
        .unwrap();
    let header = w
        .store
        .freeze_outcomes(w.f.reservation.id, now())
        .await
        .unwrap();
    let packet = w
        .store
        .outcome_packet(header.reservation.id, 0)
        .await
        .unwrap();
    let (core, task) = bridge(&w, true).await;
    let ack = core.payout_outcome(&packet).await.unwrap();
    w.store
        .acknowledge_outcomes(header.reservation.id, &ack, now())
        .await
        .unwrap();
    let failed = root.join("failed-images");
    images(&failed, &urls).await;
    let report = inspect_fixture(&failed, &p).await.unwrap();
    assert_eq!(report.unresolved_provider_payments, 0);
    assert_eq!(report.rows[&Role::Payout]["payout_outcome_acks"], 1);
    assert_eq!(report.rows[&Role::Payout]["payout_cost_acks"], 0);
    assert_eq!(report.payout_held_sats, 2);
    assert_eq!(w.store.held_sats().await.unwrap(), 2);
    let approved = ApprovedDecision::fixture(decision(&packet, 1));
    w.f.ledger
        .reconcile_failed_cost(&approved, &reader(&w))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    w.store
        .sync_failed_cost(&core, header.reservation.id, &packet.row.member)
        .await
        .unwrap()
        .unwrap();
    task.abort();
    let reviewed = root.join("reviewed-images");
    images(&reviewed, &urls).await;
    let report = inspect_fixture(&reviewed, &p).await.unwrap();
    assert_eq!(report.rows[&Role::Payout]["payout_cost_acks"], 1);
    assert_eq!(report.rows[&Role::Core]["payout_failed_costs"], 1);
    assert_eq!(report.payout_held_sats, 0);
    assert_eq!(w.store.held_sats().await.unwrap(), 0);
    assert_eq!(w.store.budget_usage_sats().await.unwrap(), 1);
    let db = SqlitePool::connect(&format!(
        "sqlite://{}",
        reviewed.join("signer.db").display()
    ))
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT event_json FROM signer_results")
            .fetch_one(&db)
            .await
            .unwrap(),
        signed
    );
    db.close().await;
    let mut wrong = p.clone();
    wrong.epoch = Uuid::new_v4();
    assert!(inspect_fixture(&reviewed, &wrong).await.is_err());
    wrong = p.clone();
    wrong.provider.account_id = Uuid::new_v4();
    assert!(inspect_fixture(&reviewed, &wrong).await.is_err());
    // Individually valid, current-schema older payout state has lost the quote
    // retained by core's execution claim. It cannot qualify as a matching set.
    std::fs::remove_file(reviewed.join("payout.db")).unwrap();
    std::fs::copy(older_payout, reviewed.join("payout.db")).unwrap();
    assert!(inspect_fixture(&reviewed, &p).await.is_err());
    assert_eq!(w.mock.lock().unwrap().patch_ids.len(), 1);
    assert_eq!(w.mock.lock().unwrap().payments.len(), 1);
    payout.close().await;
}
