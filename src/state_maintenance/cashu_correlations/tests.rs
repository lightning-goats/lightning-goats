use super::*;
use crate::cashu::receipt_authority::{
    ImportLimits, MintImportLimit, ReceiptPolicy, fixture_authenticate,
};
use crate::state_maintenance::{control, tests as fixtures};
use lightning_goats_cashu_receipt::{ReceiptData, WireReceipt};
use std::{
    os::unix::fs::PermissionsExt as _,
    sync::{Arc, atomic::AtomicBool},
};

/// Explicit root-only positive fixture; ordinary unprivileged test runs cannot
/// count a skipped authority branch as recovery qualification.
#[tokio::test]
#[ignore = "run via the selected-artifact root/network-namespace runner"]
async fn frozen_cashu_journals_preserve_native_correlations_without_restore_authority() {
    assert_eq!(crate::private_ipc::uid().unwrap(), 0);
    let root = tempfile::Builder::new()
        .prefix("lg-cashu-correlations-")
        .tempdir_in("/run")
        .unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut policy = fixtures::policy(root.path());
    for component in policy.components.values() {
        std::fs::create_dir(component.database.parent().unwrap()).unwrap();
    }
    let url = |role| format!("sqlite://{}", policy.components[&role].database.display());
    let core = crate::ledger::LedgerStore::connect(&url(Role::Core))
        .await
        .unwrap();
    policy.epoch = core.overlay_stream_id().await.unwrap();
    core.initialize_reward_funding().await.unwrap();
    core.configure_feeder_credit_cap(1000).await.unwrap();
    let receipt_policy = ReceiptPolicy {
        wallet_uid: 60001,
        wallet_instance: Uuid::new_v4(),
        wallet_epoch: Uuid::new_v4(),
        core_epoch: policy.epoch,
        import_enabled: true,
        limits: ImportLimits {
            receipts_per_24h: 10,
            unlimited_receipts_per_24h: false,
            credit_sats_per_24h: 20000,
            maximum_custody_sats: 20000,
        },
        approved: [(
            ("review-fixture".into(), "v1".into()),
            MintImportLimit {
                maximum_gross_sats: 10000,
                maximum_net_sats: 10000,
            },
        )]
        .into(),
    };
    let receipt = WireReceipt::validate(ReceiptData {
        schema_version: 1,
        wallet_instance: receipt_policy.wallet_instance,
        wallet_epoch: receipt_policy.wallet_epoch,
        acquisition_id: Uuid::new_v4(),
        transport: "cashu".into(),
        source_ref: "synthetic-review".into(),
        source_author: None,
        mint_id: "review-fixture".into(),
        unit: "sat".into(),
        gross_sats: 251,
        receive_fee_sats: 1,
        net_sats: 250,
        acquisition_evidence_digest: "ab".repeat(32),
        policy_version: "v1".into(),
        credit_pool: "herd".into(),
    })
    .unwrap();
    let peer = fixture_authenticate(&receipt_policy, &receipt.encode().unwrap()).unwrap();
    core.import_cashu_receipt(&receipt_policy, &peer)
        .await
        .unwrap();
    let url = |role| format!("sqlite://{}", policy.components[&role].database.display());
    let membership = crate::cyberherd::store::Store::connect(&url(Role::Membership))
        .await
        .unwrap();
    membership
        .initialize(
            &policy.owner,
            &crate::cyberherd::membership::Settings {
                minimum_sats: 10,
                max_members: 10,
                member_percent: 10,
            },
            0,
        )
        .await
        .unwrap();
    membership
        .initialize_core_ingress(
            &policy.owner,
            &crate::cyberherd::ingress::CoreBinding {
                core_uid: policy.components[&Role::Core].uid,
                epoch: policy.epoch,
                start_after: 0,
                credit_pool: "herd".into(),
                payees: [("herd".into(), policy.owner.clone())].into(),
            },
        )
        .await
        .unwrap();
    let payout =
        crate::payout::store::Store::connect(&url(Role::Payout), fixtures::payout_policy(&policy))
            .await
            .unwrap();
    let signer =
        crate::payout::signer::Store::connect(&url(Role::Signer), fixtures::signer_policy(&policy))
            .await
            .unwrap();
    let _idle_native_stores = (&core, &membership, &payout, &signer);
    let bundle = root.path().join("bundle");
    files::new_private_directory(&bundle).unwrap();
    let mut images = Vec::new();
    for role in Role::ALL {
        // No workers run in this fixture. VACUUM INTO preserves current schema,
        // actual Cashu receipts, and the native cross-journal bindings.
        let pool = crate::sqlite::connect_durable(&url(role), 1).await.unwrap();
        let output = bundle.join(role.file());
        sqlx::query("VACUUM INTO ?")
            .bind(output.to_str().unwrap())
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o600)).unwrap();
        let bytes = std::fs::read(&output).unwrap();
        images.push(Image {
            name: role.file(),
            bytes: bytes.len() as u64,
            sha256: files::hash_bytes(&bytes),
        });
    }
    let policy_file = root.path().join("policy.toml");
    files::write_new(&policy_file, toml::to_string(&policy).unwrap().as_bytes()).unwrap();
    let mut operator = Operator::load(&policy_file).unwrap();
    operator.control = control::Control::Fixture(Arc::new(AtomicBool::new(true)));
    let input = Input {
        version: 2,
        purpose: "cashu_four_journal_correlation_review".into(),
        id: Uuid::new_v4(),
        policy: policy.clone(),
        source_policy_sha256: operator.policy_hash.clone(),
        cashu_inventory_sha256: "cd".repeat(32),
        images,
    };
    let manifest = bundle.join("MANIFEST.json");
    let raw = serde_json::to_vec_pretty(&input).unwrap();
    files::write_new(&manifest, &raw).unwrap();
    let output = root.path().join("review.json");
    let review = operator
        .review_cashu_correlations(&bundle, &output)
        .await
        .unwrap();
    assert_eq!(review.core_image_sha256, input.images[0].sha256);
    assert_eq!(review.cashu_inventory_sha256, input.cashu_inventory_sha256);
    assert_eq!(review.input_manifest_sha256, files::hash_bytes(&raw));
    assert_eq!(
        review.four_journal_correlations.rows[&Role::Core]["cashu_receipt_inbox"],
        1
    );
    assert!(!review.cashu_wallet_verified);
    assert!(!review.combined_restore_authorized);
    assert!(!review.resume_authorized);
    assert!(!review.four_journal_correlations.resume_authorized);
    assert!(
        operator
            .review_cashu_correlations(&bundle, &output)
            .await
            .is_err()
    );
    files::check_images(&bundle, &input.images, policy.max_component_bytes).unwrap();
    assert_eq!(std::fs::read(&manifest).unwrap(), raw);
    let ordinary_error = database::inspect(&bundle, &policy)
        .await
        .unwrap_err()
        .to_string();
    assert!(ordinary_error.contains("Cashu custody requires a coordinated"));
    assert!(operator.verify(&bundle).await.is_err());
    let restore = root.path().join("restore");
    assert!(operator.stage_restore(&bundle, &restore).await.is_err());
    assert!(!restore.exists());

    // Review a historical copy with the same populated Cashu receipt.
    let legacy = root.path().join("legacy");
    files::new_private_directory(&legacy).unwrap();
    for role in Role::ALL {
        std::fs::copy(bundle.join(role.file()), legacy.join(role.file())).unwrap();
    }
    let legacy_core = legacy.join(Role::Core.file());
    let pool = crate::sqlite::connect_durable(&format!("sqlite://{}", legacy_core.display()), 1)
        .await
        .unwrap();
    sqlx::raw_sql(
        "DROP TRIGGER freeze_cashu_settled_ranges; DROP TABLE feed_cashu_proceeds; DROP TABLE cashu_settled_ranges; DROP TABLE cashu_treasury_inspections;
         DROP TABLE cashu_treasury_fee_observations;
         DELETE FROM _sqlx_migrations WHERE version IN (27,28,29);",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
    let legacy_review = database::inspect_cashu_correlations(&legacy, &policy)
        .await
        .unwrap();
    assert_eq!(legacy_review.rows[&Role::Core]["cashu_receipt_inbox"], 1);
    assert!(!legacy_review.rows[&Role::Core].contains_key("cashu_treasury_fee_observations"));
    assert!(!legacy_review.resume_authorized);

    // Every malformed scope must fail before creating a report.
    let original = serde_json::to_value(&input).unwrap();
    for case in 0..8 {
        let mut bad = original.clone();
        match case {
            0 => bad["purpose"] = "ordinary_backup".into(),
            1 => bad["version"] = 1.into(),
            2 => bad["id"] = Uuid::nil().to_string().into(),
            3 => bad["source_policy_sha256"] = "ee".repeat(32).into(),
            4 => bad["cashu_inventory_sha256"] = "invalid".into(),
            5 => {
                bad["images"].as_array_mut().unwrap().pop();
            }
            6 => bad["images"][0]["sha256"] = "ff".repeat(32).into(),
            _ => bad["policy"]["epoch"] = Uuid::new_v4().to_string().into(),
        }
        std::fs::write(&manifest, serde_json::to_vec(&bad).unwrap()).unwrap();
        let rejected = root.path().join(format!("rejected-{case}.json"));
        assert!(
            operator
                .review_cashu_correlations(&bundle, &rejected)
                .await
                .is_err()
        );
        assert!(!rejected.exists());
    }
    std::fs::write(&manifest, &raw).unwrap();
    files::write_new(&bundle.join("INCOMPLETE"), b"incomplete").unwrap();
    assert!(
        operator
            .review_cashu_correlations(&bundle, &root.path().join("incomplete.json"))
            .await
            .is_err()
    );
    std::fs::remove_file(bundle.join("INCOMPLETE")).unwrap();

    // A separately initialized legitimate journal can be from another core.
    // Preserve the immutable binding trigger; never disable it to fabricate a
    // mismatch. Rehashing that journal still cannot join it to this recovery set.
    let other_path = root.path().join("other-membership.db");
    let other_url = format!("sqlite://{}", other_path.display());
    let other = crate::cyberherd::store::Store::connect(&other_url)
        .await
        .unwrap();
    other
        .initialize(
            &policy.owner,
            &crate::cyberherd::membership::Settings {
                minimum_sats: 10,
                max_members: 10,
                member_percent: 10,
            },
            0,
        )
        .await
        .unwrap();
    other
        .initialize_core_ingress(
            &policy.owner,
            &crate::cyberherd::ingress::CoreBinding {
                core_uid: policy.components[&Role::Core].uid,
                epoch: Uuid::new_v4(),
                start_after: 0,
                credit_pool: "herd".into(),
                payees: [("herd".into(), policy.owner.clone())].into(),
            },
        )
        .await
        .unwrap();
    other.close_after_initialization().await;
    let pool = crate::sqlite::connect_durable(&other_url, 1).await.unwrap();
    let replacement = bundle.join(Role::Membership.file());
    std::fs::remove_file(&replacement).unwrap();
    sqlx::query("VACUUM INTO ?")
        .bind(replacement.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    std::fs::set_permissions(&replacement, std::fs::Permissions::from_mode(0o600)).unwrap();
    let mut changed = input;
    let bytes = std::fs::read(bundle.join(Role::Membership.file())).unwrap();
    let image = changed
        .images
        .iter_mut()
        .find(|image| image.name == Role::Membership.file())
        .unwrap();
    image.bytes = bytes.len() as u64;
    image.sha256 = files::hash_bytes(&bytes);
    std::fs::write(&manifest, serde_json::to_vec(&changed).unwrap()).unwrap();
    let rejected = root.path().join("wrong-epoch.json");
    let error = operator
        .review_cashu_correlations(&bundle, &rejected)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("membership cursor or epoch"), "{error}");
    assert!(!rejected.exists());
    println!("CASHU_FOUR_JOURNAL_CORRELATIONS_VERIFIED");
}
