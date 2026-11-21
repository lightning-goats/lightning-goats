//! Strict additive Cashu comparison of separate four-journal images.
//! It does not migrate a journal, restore state or authorize service resumption.
use std::{collections::BTreeMap, path::Path};

use anyhow::{Result, bail};
use sqlx::sqlite::SqliteConnectOptions;

use super::{Manifest, Operator, Policy, Role, database, files, legacy::rows};

use super::contact_upgrade::{Definition, catalog};
// Child-first order also supports removal of empty additions in historical test fixtures.
pub(super) const CASHU_TABLES: [&str; 24] = [
    "feed_cashu_proceeds",
    "cashu_settled_ranges",
    "cashu_treasury_inspections",
    "cashu_treasury_fee_observations",
    "cashu_sweep_jobs",
    "cashu_sweep_requests",
    "feed_financial_treasury",
    "feed_financial_supersessions",
    "feed_financial_parts",
    "feed_financial_plans",
    "cashu_treasury_work",
    "cashu_treasury_source_results",
    "cashu_treasury_completions",
    "cashu_treasury_wallet_results",
    "cashu_treasury_deposits",
    "cashu_treasury_issuances",
    "cashu_treasury_claims",
    "cashu_treasury_sources",
    "cashu_treasury_intents",
    "cashu_treasury_clock",
    "cashu_receipt_inbox",
    "cashu_import_clock",
    "feed_source_parts",
    "feed_source_reservations",
];
pub(super) fn tables_for_core(version: i64) -> impl Iterator<Item = &'static str> {
    CASHU_TABLES
        .into_iter()
        .chain([
            "breez_treasury_inspections",
            "breez_treasury_wallet_results",
            "cashu_payout_execution_claims",
            "retained_cashu_lots",
            "breez_collected_claims",
            "breez_project_invoices",
        ])
        .filter(move |name| match *name {
            "breez_treasury_inspections" => version >= 33,
            "breez_treasury_wallet_results" => version >= 32,
            "cashu_payout_execution_claims" => version >= 31,
            "retained_cashu_lots" | "breez_collected_claims" | "breez_project_invoices" => {
                version >= 30
            }
            "feed_cashu_proceeds" | "cashu_settled_ranges" => version >= 29,
            "cashu_treasury_inspections" => version >= 28,
            "cashu_treasury_fee_observations" => version >= 27,
            _ => true,
        })
}

pub(super) async fn private_catalog(version: i64) -> Result<BTreeMap<String, Definition>> {
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
        sqlx::raw_sql(include_str!("../../migrations/0026_cashu_receipts.sql"))
            .execute(&pool)
            .await?;
        if version >= 27 {
            sqlx::raw_sql(include_str!(
                "../../migrations/0027_cashu_fee_observations.sql"
            ))
            .execute(&pool)
            .await?;
        }
        if version >= 28 {
            sqlx::raw_sql(include_str!(
                "../../migrations/0028_cashu_treasury_inspections.sql"
            ))
            .execute(&pool)
            .await?;
        }
        if version >= 29 {
            sqlx::raw_sql(include_str!(
                "../../migrations/0029_cashu_feed_proceeds.sql"
            ))
            .execute(&pool)
            .await?;
        }
        Ok(catalog(&pool)
            .await?
            .into_iter()
            .filter(|(_, d)| CASHU_TABLES.contains(&d.table.as_str()))
            .collect())
    }
    .await;
    pool.close().await;
    result
}

/// Apply only the catalog changes produced by the exact checked-in Breez migration.
/// A changed legacy definition must match its original pinned definition first.
pub(super) async fn apply_breez_catalog(
    expected: &mut BTreeMap<String, Definition>,
    version: i64,
) -> Result<()> {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;
    let migrations = sqlx::migrate!("./migrations");
    for migration in migrations.iter().take_while(|m| m.version < 30) {
        sqlx::raw_sql(&migration.sql).execute(&pool).await?;
    }
    let old = catalog(&pool).await?;
    for migration in migrations
        .iter()
        .filter(|m| (30..=version).contains(&m.version))
    {
        sqlx::raw_sql(&migration.sql).execute(&pool).await?;
    }
    let new = catalog(&pool).await?;
    for (key, definition) in &old {
        if new.get(key) != Some(definition) && expected.remove(key).as_ref() != Some(definition) {
            bail!("unexpected original schema for Breez migration");
        }
    }
    for (key, definition) in new {
        if old.get(&key) != Some(&definition) && expected.insert(key, definition).is_some() {
            bail!("Breez migration catalog collision");
        }
    }
    pool.close().await;
    Ok(())
}

pub(super) async fn compare_images(before: &Path, after: &Path, policy: &Policy) -> Result<()> {
    database::inspect_schema_25(before, policy).await?;
    let (_, version) = database::inspect_upgrade_copy(after, policy, 26, false).await?;
    let expected_private = private_catalog(version).await?;
    for role in Role::ALL {
        let old = rows::open_named(before, &role.file()).await?;
        let new = rows::open_named(after, &role.file()).await?;
        let result: Result<()> = async {
            let old_names = rows::names(&old).await?;
            let mut expected_names = old_names.clone();
            if role == Role::Core {
                expected_names.extend(tables_for_core(version).map(str::to_owned));
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
            if role == Role::Core {
                if rows::cashu_prefix_digest(&old).await? != rows::cashu_prefix_digest(&new).await?
                {
                    bail!("upgrade changed existing migration history");
                }
            } else if rows::digest(&old, "_sqlx_migrations", &migration_columns).await?
                != rows::digest(&new, "_sqlx_migrations", &migration_columns).await?
            {
                bail!("upgrade changed another role's migration history");
            }
            let mut expected = catalog(&old).await?;
            if role == Role::Core {
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
                        bail!("new Cashu schema collides with an existing object");
                    }
                }
            }
            if role == Role::Core && version >= 30 {
                super::cashu_upgrade::apply_breez_catalog(&mut expected, version).await?;
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
    /// Reverify only an exact schema-25 upgrade source. Does not make that bundle
    /// eligible for ordinary current-schema verification or restore staging.
    pub async fn verify_cashu_upgrade_source(&self, bundle: &Path) -> Result<Manifest> {
        files::private_bundle(bundle)?;
        match std::fs::symlink_metadata(bundle.join("INCOMPLETE")) {
            Ok(_) => bail!("Cashu upgrade input is incomplete"),
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
            bail!("Cashu upgrade source scope changed");
        }
        files::check_images(bundle, &manifest.images, self.policy.max_component_bytes)?;
        if database::inspect_schema_25(bundle, &self.policy).await? != manifest.report {
            bail!("Cashu upgrade source report changed");
        }
        files::check_images(bundle, &manifest.images, self.policy.max_component_bytes)?;
        Ok(manifest)
    }

    /// Accept only a source-pinned schema-25 backup and a separate, already
    /// migrated four-journal copy. Normal verify/restore still require the current schema.
    pub async fn compare_cashu_upgrade(&self, bundle: &Path, candidate: &Path) -> Result<Manifest> {
        let manifest = self.verify_cashu_upgrade_source(bundle).await?;
        files::private_bundle(candidate)?;
        match std::fs::symlink_metadata(candidate.join("INCOMPLETE")) {
            Ok(_) => bail!("Cashu upgrade input is incomplete"),
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
    #[tokio::test]
    async fn inactive_cashu_upgrade_preserves_all_paid_signed_and_pending_history() {
        let root = tempfile::TempDir::new().unwrap();
        let (policy, _keep) = super::super::contact_upgrade::tests::seed(root.path()).await;
        let pool = crate::sqlite::connect_durable(
            &format!(
                "sqlite://{}",
                policy.components[&Role::Core].database.display()
            ),
            1,
        )
        .await
        .unwrap();
        database::migrate_core_through(&pool, 25).await.unwrap();
        pool.close().await;
        let before = root.path().join("before");
        let after = root.path().join("after");
        super::super::contact_upgrade::tests::copy_images(&policy, &before);
        super::super::contact_upgrade::tests::copy_images(&policy, &after);
        database::inspect_schema_25(&before, &policy).await.unwrap();
        assert!(database::inspect(&before, &policy).await.is_err());
        let pool = crate::sqlite::connect_durable(
            &format!("sqlite://{}", after.join("core.db").display()),
            1,
        )
        .await
        .unwrap();
        database::migrate_core_through(&pool, 26).await.unwrap();
        compare_images(&before, &after, &policy).await.unwrap();
        database::migrate_core_through(&pool, database::CURRENT_CORE_SCHEMA)
            .await
            .unwrap();
        compare_images(&before, &after, &policy).await.unwrap();
        sqlx::query("UPDATE ledger_entries SET delta_sats=delta_sats+1 WHERE id=1")
            .execute(&pool)
            .await
            .unwrap();
        assert!(compare_images(&before, &after, &policy).await.is_err());
        sqlx::query("UPDATE ledger_entries SET delta_sats=delta_sats-1 WHERE id=1")
            .execute(&pool)
            .await
            .unwrap();
        compare_images(&before, &after, &policy).await.unwrap();
        sqlx::query("CREATE TRIGGER unreviewed_cashu AFTER INSERT ON cashu_import_clock BEGIN SELECT 1; END").execute(&pool).await.unwrap();
        assert!(compare_images(&before, &after, &policy).await.is_err());
        sqlx::query("DROP TRIGGER unreviewed_cashu")
            .execute(&pool)
            .await
            .unwrap();
        compare_images(&before, &after, &policy).await.unwrap();
        sqlx::query("INSERT INTO cashu_import_clock VALUES(1,100)")
            .execute(&pool)
            .await
            .unwrap();
        assert!(compare_images(&before, &after, &policy).await.is_err());
        pool.close().await;
    }
}
