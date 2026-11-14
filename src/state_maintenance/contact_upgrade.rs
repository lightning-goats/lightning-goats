//! Strict schema-23 -> 25 (contact plus qualified feeder-cap schema) additive comparison of separate four-journal images.
//! It does not migrate a journal, restore state or authorize service resumption.
use std::{collections::BTreeMap, path::Path};

use anyhow::{Result, bail};
use futures_util::TryStreamExt as _;
use sha2::{Digest as _, Sha256};
use sqlx::{Row as _, SqlitePool, sqlite::SqliteConnectOptions};

use super::{Manifest, Operator, Policy, Role, database, files, legacy::rows};

const PRIVATE_TABLES: [&str; 3] = [
    "private_contact_intents",
    "private_contact_fees",
    "private_contact_delivery",
];
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Definition {
    pub(super) table: String,
    pub(super) sha256: String,
}
pub(super) async fn catalog(pool: &SqlitePool) -> Result<BTreeMap<String, Definition>> {
    let mut result = BTreeMap::new();
    let mut stream = sqlx::query(
        "SELECT type,name,tbl_name,sql FROM sqlite_master ORDER BY type,name LIMIT 2049",
    )
    .fetch(pool);
    let mut bytes = 0usize;
    while let Some(row) = stream.try_next().await? {
        if result.len() >= 2048 {
            bail!("journal catalog exceeds object bound");
        }
        let kind: String = row.try_get("type")?;
        let name: String = row.try_get("name")?;
        let table: String = row.try_get("tbl_name")?;
        let definition: Option<String> = row.try_get("sql")?;
        if kind.len() > 16 || name.len() > 128 || table.len() > 128 {
            bail!("journal catalog identifier exceeds bound");
        }
        let encoded = serde_json::to_vec(&(&kind, &name, &table, &definition))?;
        bytes = bytes.saturating_add(encoded.len());
        if bytes > 4 * 1024 * 1024 {
            bail!("journal catalog exceeds byte bound");
        }
        result.insert(
            format!("{kind}:{name}"),
            Definition {
                table,
                sha256: hex::encode(Sha256::digest(encoded)),
            },
        );
    }
    Ok(result)
}

async fn private_catalog() -> Result<BTreeMap<String, Definition>> {
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
        sqlx::raw_sql(include_str!("../../migrations/0024_private_contacts.sql"))
            .execute(&pool)
            .await?;
        catalog(&pool).await
    }
    .await;
    pool.close().await;
    result
}

pub(super) async fn compare_images(before: &Path, after: &Path, policy: &Policy) -> Result<()> {
    database::inspect_before_contact(before, policy).await?;
    let migrated = database::inspect(after, policy).await?;
    for table in PRIVATE_TABLES {
        if migrated.rows[&Role::Core].get(table) != Some(&0) {
            bail!("contact upgrade must add empty private tables only");
        }
    }
    let mut expected_private = private_catalog().await?;
    expected_private.extend(super::feeder_cap_upgrade::private_catalog().await?);
    for table in ["feeder_credit_policy", "feed_credit_retirements"] {
        if migrated.rows[&Role::Core].get(table) != Some(&0) {
            bail!("contact upgrade must not activate feeder policy or retire credit");
        }
    }
    for role in Role::ALL {
        let old = rows::open_named(before, &role.file()).await?;
        let new = rows::open_named(after, &role.file()).await?;
        let result: Result<()> = async {
            if role == Role::Core {
                super::feeder_cap_upgrade::verify_backfills(&old, &new).await?;
            }
            let old_names = rows::names(&old).await?;
            let mut expected_names = old_names.clone();
            if role == Role::Core {
                expected_names.extend(PRIVATE_TABLES.map(str::to_owned));
                expected_names.extend(super::feeder_cap_upgrade::CAP_TABLES.map(str::to_owned));
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
                if rows::migration_prefix_digest(&old).await?
                    != rows::migration_prefix_digest(&new).await?
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
                        bail!("new contact schema collides with an existing object");
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
    /// Reverify only an exact schema-23 upgrade source. Does not make that bundle
    /// eligible for ordinary current-schema verification or restore staging.
    pub async fn verify_contact_upgrade_source(&self, bundle: &Path) -> Result<Manifest> {
        files::private_bundle(bundle)?;
        match std::fs::symlink_metadata(bundle.join("INCOMPLETE")) {
            Ok(_) => bail!("contact upgrade input is incomplete"),
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
            bail!("contact upgrade source scope changed");
        }
        files::check_images(bundle, &manifest.images, self.policy.max_component_bytes)?;
        if database::inspect_before_contact(bundle, &self.policy).await? != manifest.report {
            bail!("contact upgrade source report changed");
        }
        files::check_images(bundle, &manifest.images, self.policy.max_component_bytes)?;
        Ok(manifest)
    }

    /// Accept only a source-pinned schema-23 backup and a separate, already
    /// migrated four-journal copy. Normal verify/restore still require schema 25.
    pub async fn compare_contact_upgrade(
        &self,
        bundle: &Path,
        candidate: &Path,
    ) -> Result<Manifest> {
        let manifest = self.verify_contact_upgrade_source(bundle).await?;
        files::private_bundle(candidate)?;
        match std::fs::symlink_metadata(candidate.join("INCOMPLETE")) {
            Ok(_) => bail!("contact upgrade input is incomplete"),
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
#[path = "contact_upgrade_tests.rs"]
pub(super) mod tests;
