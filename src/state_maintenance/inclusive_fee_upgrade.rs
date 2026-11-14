//! Strict payout-schema-8 -> 11 additive comparison of separate four-journal images.
//! It does not migrate a journal, restore state or authorize service resumption.
use std::{collections::BTreeMap, path::Path};

use anyhow::{Result, bail};
use sqlx::sqlite::SqliteConnectOptions;

use super::{Manifest, Operator, Policy, Role, database, files, legacy::rows};

use super::contact_upgrade::{Definition, catalog};
const FEE_TABLES: [&str; 7] = [
    "payout_fee_observations",
    "payout_fee_probe_intents",
    "payout_fee_probe_callbacks",
    "payout_fee_probe_invoices",
    "payout_fee_probe_quotes",
    "payout_fee_promotions",
    "payout_fee_probe_duplicates",
];
pub(super) async fn private_catalog() -> Result<BTreeMap<String, Definition>> {
    // Static schema definitions only. No payment, private envelope, journal or
    // credential is ever loaded into this disposable reference database.
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(":memory:")
                .foreign_keys(true)
                .pragma("trusted_schema", "OFF")
                .pragma("hard_heap_limit", "268435456"),
        )
        .await?;
    let result = async {
        sqlx::migrate!("./payout-migrations").run(&pool).await?;
        Ok(catalog(&pool)
            .await?
            .into_iter()
            .filter(|(_, d)| FEE_TABLES.contains(&d.table.as_str()))
            .collect())
    }
    .await;
    pool.close().await;
    result
}

pub(super) async fn compare_images(before: &Path, after: &Path, policy: &Policy) -> Result<()> {
    database::inspect_before_inclusive_fees(before, policy).await?;
    let migrated = database::inspect(after, policy).await?;
    for table in FEE_TABLES {
        if migrated.rows[&Role::Payout].get(table) != Some(&0) {
            bail!("inclusive-fee upgrade must add empty fee tables only");
        }
    }
    let expected_private = private_catalog().await?;
    for role in Role::ALL {
        let old = rows::open_named(before, &role.file()).await?;
        let new = rows::open_named(after, &role.file()).await?;
        let result: Result<()> = async {
            let old_names = rows::names(&old).await?;
            let mut expected_names = old_names.clone();
            if role == Role::Payout {
                expected_names.extend(FEE_TABLES.map(str::to_owned));
                expected_names.sort();
            }
            if rows::names(&new).await? != expected_names {
                bail!("upgrade changed the original journal table set");
            }
            for table in old_names {
                let columns = rows::columns(&old, &table).await?;
                if rows::columns(&new, &table).await? != columns
                    || rows::digest(&old, &table, &columns).await?
                        != rows::digest(&new, &table, &columns).await?
                {
                    bail!(
                        "upgrade changed retained journal rows, row IDs, columns or signed bytes"
                    );
                }
            }
            let migration_columns = rows::columns(&old, "_sqlx_migrations").await?;
            if rows::columns(&new, "_sqlx_migrations").await? != migration_columns {
                bail!("upgrade changed migration-history columns");
            }
            if role == Role::Payout {
                if rows::inclusive_fee_prefix_digest(&old).await?
                    != rows::inclusive_fee_prefix_digest(&new).await?
                {
                    bail!("upgrade changed existing migration history");
                }
            } else if rows::digest(&old, "_sqlx_migrations", &migration_columns).await?
                != rows::digest(&new, "_sqlx_migrations", &migration_columns).await?
            {
                bail!("upgrade changed another role's migration history");
            }
            let mut expected = catalog(&old).await?;
            if role == Role::Payout {
                for (key, value) in &expected_private {
                    if expected
                        .insert(
                            key.clone(),
                            Definition {
                                table: value.table.clone(),
                                sha256: value.sha256.clone(),
                            },
                        )
                        .is_some()
                    {
                        bail!("new fee schema collides with an existing object");
                    }
                }
            }
            if catalog(&new).await? != expected {
                bail!("upgrade changed original schema or added unqualified private definitions");
            }
            Ok(())
        }
        .await;
        old.close().await;
        new.close().await;
        result?;
    }
    Ok(())
}

impl Operator {
    /// Reverify only an exact payout-schema-8 upgrade source. Does not make that bundle
    /// eligible for ordinary current-schema verification or restore staging.
    pub async fn verify_inclusive_fee_upgrade_source(&self, bundle: &Path) -> Result<Manifest> {
        files::private_bundle(bundle)?;
        match std::fs::symlink_metadata(bundle.join("INCOMPLETE")) {
            Ok(_) => bail!("inclusive-fee upgrade input is incomplete"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let raw = files::read_private(&bundle.join("MANIFEST.json"), 65536)?;
        let manifest: Manifest = serde_json::from_slice(&raw)
            .map_err(|_| anyhow::anyhow!("invalid private source manifest"))?;
        if manifest.version != 1
            || manifest.id.is_nil()
            || manifest.created_at < 0
            || manifest.policy != self.policy
            || manifest.source_policy_sha256 != self.policy_hash
            || !manifest.recovery_required
        {
            bail!("inclusive-fee upgrade source scope changed");
        }
        files::check_images(bundle, &manifest.images, self.policy.max_component_bytes)?;
        if database::inspect_before_inclusive_fees(bundle, &self.policy).await? != manifest.report {
            bail!("inclusive-fee upgrade source report changed");
        }
        files::check_images(bundle, &manifest.images, self.policy.max_component_bytes)?;
        Ok(manifest)
    }

    /// Accept only a source-pinned payout-schema-8 backup and a separate, already
    /// migrated four-journal copy. Normal verify/restore still require current compiled schemas.
    pub async fn compare_inclusive_fee_upgrade(
        &self,
        bundle: &Path,
        candidate: &Path,
    ) -> Result<Manifest> {
        let manifest = self.verify_inclusive_fee_upgrade_source(bundle).await?;
        files::private_bundle(candidate)?;
        match std::fs::symlink_metadata(candidate.join("INCOMPLETE")) {
            Ok(_) => bail!("inclusive-fee upgrade input is incomplete"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let mut images = Vec::new();
        for role in Role::ALL {
            for name in [role.file(), format!("{}-wal", role.file())] {
                match std::fs::symlink_metadata(candidate.join(&name)) {
                    Ok(_) => images.push(files::private_image(
                        &candidate.join(&name),
                        self.policy.max_component_bytes,
                    )?),
                    Err(e)
                        if e.kind() == std::io::ErrorKind::NotFound && name.ends_with("-wal") => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        files::check_images(candidate, &images, self.policy.max_component_bytes)?;
        compare_images(bundle, candidate, &self.policy).await?;
        files::check_images(candidate, &images, self.policy.max_component_bytes)?;
        files::check_images(bundle, &manifest.images, self.policy.max_component_bytes)?;
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_maintenance::contact_upgrade::tests::copy_images;
    async fn seed(
        root: &Path,
    ) -> (
        Policy,
        crate::state_maintenance::contact_upgrade::tests::Keep,
    ) {
        let (policy, keep) = super::super::contact_upgrade::tests::seed(root).await;
        let core = crate::ledger::LedgerStore::connect(&format!(
            "sqlite://{}",
            policy.components[&Role::Core].database.display()
        ))
        .await
        .unwrap();
        drop(core);
        let payout_url = format!(
            "sqlite://{}",
            policy.components[&Role::Payout].database.display()
        );
        let payout = crate::sqlite::connect_durable(&payout_url, 1)
            .await
            .unwrap();
        // Fixture-only demotion of empty additive tables; no runtime command
        // can remove schema or migration records from an actual paid journal.
        for table in [
            "payout_fee_probe_duplicates",
            "payout_fee_promotions",
            "payout_fee_probe_quotes",
            "payout_fee_probe_invoices",
            "payout_fee_probe_callbacks",
            "payout_fee_probe_intents",
            "payout_fee_observations",
        ] {
            let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&payout)
                .await
                .unwrap();
            assert_eq!(count, 0);
            sqlx::query(&format!("DROP TABLE {table}"))
                .execute(&payout)
                .await
                .unwrap();
        }
        sqlx::query("DELETE FROM _sqlx_migrations WHERE version>=9")
            .execute(&payout)
            .await
            .unwrap();
        payout.close().await;
        (policy, keep)
    }
    #[tokio::test]
    async fn fee_upgrade_retains_all_journals_and_rejects_extra_schema_or_changed_paid_rows() {
        let root = tempfile::TempDir::new().unwrap();
        let (policy, _keep) = seed(root.path()).await;
        let before = root.path().join("before");
        let after = root.path().join("after");
        super::super::contact_upgrade::tests::copy_images(&policy, &before);
        super::super::contact_upgrade::tests::copy_images(&policy, &after);
        database::inspect_before_inclusive_fees(&before, &policy)
            .await
            .unwrap();
        assert!(database::inspect(&before, &policy).await.is_err());
        let migrated = crate::sqlite::connect_durable(
            &format!("sqlite://{}", after.join("payout.db").display()),
            1,
        )
        .await
        .unwrap();
        sqlx::migrate!("./payout-migrations")
            .run(&migrated)
            .await
            .unwrap();
        compare_images(&before, &after, &policy).await.unwrap();
        sqlx::query("CREATE TRIGGER unexpected_fee_trigger AFTER INSERT ON payout_fee_probe_intents BEGIN SELECT 1; END").execute(&migrated).await.unwrap();
        assert!(compare_images(&before, &after, &policy).await.is_err());
        sqlx::query("DROP TRIGGER unexpected_fee_trigger")
            .execute(&migrated)
            .await
            .unwrap();
        compare_images(&before, &after, &policy).await.unwrap();
        let core = crate::sqlite::connect_durable(
            &format!("sqlite://{}", after.join("core.db").display()),
            1,
        )
        .await
        .unwrap();
        // Whitespace preserves parsed Nostr content/signature, but the frozen
        // serialized outbox bytes must still be retained exactly.
        let signed: String =
            sqlx::query_scalar("SELECT signed_event_json FROM message_outbox LIMIT 1")
                .fetch_one(&core)
                .await
                .unwrap();
        sqlx::query("UPDATE message_outbox SET signed_event_json=signed_event_json||' '")
            .execute(&core)
            .await
            .unwrap();
        assert!(compare_images(&before, &after, &policy).await.is_err());
        sqlx::query("UPDATE message_outbox SET signed_event_json=?")
            .bind(&signed)
            .execute(&core)
            .await
            .unwrap();
        compare_images(&before, &after, &policy).await.unwrap();
        let description: String =
            sqlx::query_scalar("SELECT description FROM _sqlx_migrations WHERE version=8")
                .fetch_one(&migrated)
                .await
                .unwrap();
        sqlx::query(
            "UPDATE _sqlx_migrations SET description=description||' changed' WHERE version=8",
        )
        .execute(&migrated)
        .await
        .unwrap();
        assert!(compare_images(&before, &after, &policy).await.is_err());
        sqlx::query("UPDATE _sqlx_migrations SET description=? WHERE version=8")
            .bind(&description)
            .execute(&migrated)
            .await
            .unwrap();
        compare_images(&before, &after, &policy).await.unwrap();
        sqlx::query("UPDATE ledger_entries SET delta_sats=delta_sats+1 WHERE id=1")
            .execute(&core)
            .await
            .unwrap();
        assert!(compare_images(&before, &after, &policy).await.is_err());
        core.close().await;
        migrated.close().await;
    }

    /// Required separately in disposable CI under actual root file authority.
    #[tokio::test]
    #[ignore = "required isolated root inclusive-fee upgrade fixture"]
    async fn root_inclusive_fee_upgrade_compares_private_images_and_retains_original_backup() {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(crate::private_ipc::uid().unwrap(), 0);
        let root = tempfile::Builder::new()
            .prefix("lg-inclusive-fee-upgrade-")
            .tempdir_in("/run")
            .unwrap();
        let (p, _keep) = seed(root.path()).await;
        let policy_path = root.path().join("policy.toml");
        files::write_new(&policy_path, toml::to_string(&p).unwrap().as_bytes()).unwrap();
        let mut operator = Operator::load(&policy_path).unwrap();
        let ready = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        operator.control = crate::state_maintenance::control::Control::Fixture(ready.clone());
        let before = root.path().join("before");
        let after = root.path().join("after");
        copy_images(&p, &after);
        let protect = |directory: &Path| {
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
            for entry in std::fs::read_dir(directory).unwrap() {
                std::fs::set_permissions(
                    entry.unwrap().path(),
                    std::fs::Permissions::from_mode(0o600),
                )
                .unwrap();
            }
        };
        protect(&after);
        let mut originals = Vec::new();
        for component in p.components.values() {
            let parent = component.database.parent().unwrap();
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).unwrap();
            std::os::unix::fs::chown(parent, Some(component.uid), Some(component.gid)).unwrap();
            for suffix in ["", "-wal", "-shm"] {
                let path =
                    std::path::PathBuf::from(format!("{}{suffix}", component.database.display()));
                if path.exists() {
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                        .unwrap();
                    std::os::unix::fs::chown(&path, Some(component.uid), Some(component.gid))
                        .unwrap();
                    originals.push((path.clone(), std::fs::read(path).unwrap()));
                }
            }
        }
        assert!(
            operator
                .snapshot_before_inclusive_fees(&before)
                .await
                .is_err()
        );
        assert!(!before.exists());
        ready.store(true, std::sync::atomic::Ordering::SeqCst);
        // The explicit upgrade snapshot uses the same actual four-owner source
        // files, stopped-writer observer and unchanged WAL checks as normal backup.
        // Only its exact payout-prefix inspection differs; normal restore stays closed.
        let manifest = operator
            .snapshot_before_inclusive_fees(&before)
            .await
            .unwrap();
        assert_eq!(
            operator
                .verify_inclusive_fee_upgrade_source(&before)
                .await
                .unwrap(),
            manifest
        );
        assert_eq!(manifest.report.pending_feeds, 1);
        assert!(
            manifest
                .images
                .iter()
                .any(|i| i.name.ends_with("-wal") && i.bytes > 0)
        );
        for (path, bytes) in &originals {
            assert_eq!(std::fs::read(path).unwrap(), *bytes);
        }
        assert!(
            operator
                .snapshot_before_inclusive_fees(&before)
                .await
                .is_err()
        );
        let refused_restore = root.path().join("refused-restore");
        assert!(
            operator
                .stage_restore(&before, &refused_restore)
                .await
                .is_err()
        );
        assert!(!refused_restore.exists());
        let original_manifest = std::fs::read(before.join("MANIFEST.json")).unwrap();
        assert!(operator.verify(&before).await.is_err());
        assert!(
            operator
                .compare_inclusive_fee_upgrade(&before, &after)
                .await
                .is_err()
        );
        let pool = crate::sqlite::connect_durable(
            &format!("sqlite://{}", after.join("payout.db").display()),
            1,
        )
        .await
        .unwrap();
        sqlx::migrate!("./payout-migrations")
            .run(&pool)
            .await
            .unwrap();
        pool.close().await;
        protect(&after);
        assert_eq!(
            operator
                .compare_inclusive_fee_upgrade(&before, &after)
                .await
                .unwrap(),
            manifest
        );
        assert!(!manifest.report.resume_authorized);
        let alias = root.path().join("aliased-candidate");
        std::os::unix::fs::symlink(&after, &alias).unwrap();
        assert!(
            operator
                .compare_inclusive_fee_upgrade(&before, &alias)
                .await
                .is_err()
        );
        let hardlink = root.path().join("aliased-core.db");
        std::fs::hard_link(after.join("core.db"), &hardlink).unwrap();
        assert!(
            operator
                .compare_inclusive_fee_upgrade(&before, &after)
                .await
                .is_err()
        );
        std::fs::remove_file(hardlink).unwrap();
        files::write_new(&after.join("unlisted"), b"unqualified").unwrap();
        assert!(
            operator
                .compare_inclusive_fee_upgrade(&before, &after)
                .await
                .is_err()
        );
        std::fs::remove_file(after.join("unlisted")).unwrap();
        files::write_new(&after.join("INCOMPLETE"), b"not accepted").unwrap();
        assert!(
            operator
                .compare_inclusive_fee_upgrade(&before, &after)
                .await
                .is_err()
        );
        std::fs::remove_file(after.join("INCOMPLETE")).unwrap();
        std::fs::set_permissions(
            after.join("core.db"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(
            operator
                .compare_inclusive_fee_upgrade(&before, &after)
                .await
                .is_err()
        );
        std::fs::set_permissions(
            after.join("core.db"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        assert_eq!(
            operator
                .compare_inclusive_fee_upgrade(&before, &after)
                .await
                .unwrap(),
            manifest
        );
        assert_eq!(
            std::fs::read(before.join("MANIFEST.json")).unwrap(),
            original_manifest
        );
        files::check_images(&before, &manifest.images, p.max_component_bytes).unwrap();
        for (path, bytes) in &originals {
            assert_eq!(std::fs::read(path).unwrap(), *bytes);
        }
        println!("ROOT_INCLUSIVE_FEE_UPGRADE_FIXTURE_VERIFIED");
    }
}
