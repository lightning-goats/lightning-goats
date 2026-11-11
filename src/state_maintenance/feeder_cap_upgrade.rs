//! Strict schema-24 -> 25 additive comparison of separate four-journal images.
//! It does not migrate a journal, restore state or authorize service resumption.
use std::{collections::BTreeMap, path::Path};

use anyhow::{Result, bail};
use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};

use super::{Manifest, Operator, Policy, Role, database, files, legacy::rows};

use super::contact_upgrade::{Definition, catalog};
pub(super) const CAP_TABLES: [&str; 4] = [
    "feeder_credit_policy",
    "feed_credit_retirements",
    "feeding_announcement_sources",
    "message_outbox_suppressions",
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
        sqlx::raw_sql("CREATE TABLE ledger_entries(id INTEGER PRIMARY KEY, delta_sats INTEGER, entry_type TEXT, source_key TEXT, feed_attempt_id TEXT); CREATE TABLE event_log(seq INTEGER PRIMARY KEY, event_type TEXT, payload_json TEXT); CREATE TABLE feed_event_sources(event_seq INTEGER); CREATE TABLE message_outbox(event_id TEXT PRIMARY KEY, source_event_seq INTEGER,status TEXT);")
            .execute(&pool).await?;
        sqlx::raw_sql(include_str!("../../migrations/0025_payment_feeding_cap.sql"))
            .execute(&pool)
            .await?;
        Ok(catalog(&pool).await?.into_iter().filter(|(_,d)| CAP_TABLES.contains(&d.table.as_str())).collect())
    }
    .await;
    pool.close().await;
    result
}

pub(super) async fn compare_images(before: &Path, after: &Path, policy: &Policy) -> Result<()> {
    database::inspect_before_feeder_cap(before, policy).await?;
    let migrated = database::inspect(after, policy).await?;
    for table in ["feeder_credit_policy", "feed_credit_retirements"] {
        if migrated.rows[&Role::Core].get(table) != Some(&0) {
            bail!(
                "feeder-cap upgrade must add no policy or retirement before explicit activation/repair"
            );
        }
    }
    let expected_private = private_catalog().await?;
    for role in Role::ALL {
        let old = rows::open_named(before, &role.file()).await?;
        let new = rows::open_named(after, &role.file()).await?;
        let result: Result<()> = async {
            if role == Role::Core {
                verify_backfills(&old, &new).await?;
            }
            let old_names = rows::names(&old).await?;
            let mut expected_names = old_names.clone();
            if role == Role::Core {
                expected_names.extend(CAP_TABLES.map(str::to_owned));
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
                if rows::feeder_cap_prefix_digest(&old).await?
                    != rows::feeder_cap_prefix_digest(&new).await?
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
    /// Reverify only an exact schema-24 upgrade source. Does not make that bundle
    /// eligible for ordinary current-schema verification or restore staging.
    pub async fn verify_feeder_cap_upgrade_source(&self, bundle: &Path) -> Result<Manifest> {
        files::private_bundle(bundle)?;
        match std::fs::symlink_metadata(bundle.join("INCOMPLETE")) {
            Ok(_) => bail!("feeder-cap upgrade input is incomplete"),
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
            bail!("feeder-cap upgrade source scope changed");
        }
        files::check_images(bundle, &manifest.images, self.policy.max_component_bytes)?;
        if database::inspect_before_feeder_cap(bundle, &self.policy).await? != manifest.report {
            bail!("feeder-cap upgrade source report changed");
        }
        files::check_images(bundle, &manifest.images, self.policy.max_component_bytes)?;
        Ok(manifest)
    }

    /// Accept only a source-pinned schema-24 backup and a separate, already
    /// migrated four-journal copy. Normal verify/restore still require schema 25.
    pub async fn compare_feeder_cap_upgrade(
        &self,
        bundle: &Path,
        candidate: &Path,
    ) -> Result<Manifest> {
        let manifest = self.verify_feeder_cap_upgrade_source(bundle).await?;
        files::private_bundle(candidate)?;
        match std::fs::symlink_metadata(candidate.join("INCOMPLETE")) {
            Ok(_) => bail!("feeder-cap upgrade input is incomplete"),
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

pub(super) async fn verify_backfills(old: &SqlitePool, new: &SqlitePool) -> Result<()> {
    // Compare deterministic derived rows against the retained old paid/event
    // journal. No caller-provided backfill and no rewrite of signed bytes.
    let expected: Vec<(i64,i64)> = sqlx::query_as("SELECT l.id,MIN(e.seq) FROM feed_event_sources s JOIN event_log e ON e.seq=s.event_seq JOIN ledger_entries l ON l.id=json_extract(e.payload_json,'$.triggering_credit_id') WHERE e.event_type='feeder_confirmed' AND l.delta_sats>0 AND json_type(e.payload_json,'$.triggering_credit_id')='integer' GROUP BY l.id ORDER BY l.id LIMIT 100001")
        .fetch_all(old).await?;
    let actual: Vec<(i64,i64)> = sqlx::query_as("SELECT credit_ledger_id,first_event_seq FROM feeding_announcement_sources ORDER BY credit_ledger_id LIMIT 100001").fetch_all(new).await?;
    if expected.len() > 100000 || actual != expected {
        bail!("first-announcement backfill changed");
    }
    let expected: Vec<String> = sqlx::query_scalar("SELECT o.event_id FROM message_outbox o JOIN event_log e ON e.seq=o.source_event_seq JOIN (SELECT l.id,MIN(e.seq) first_seq FROM feed_event_sources s JOIN event_log e ON e.seq=s.event_seq JOIN ledger_entries l ON l.id=json_extract(e.payload_json,'$.triggering_credit_id') WHERE e.event_type='feeder_confirmed' AND l.delta_sats>0 AND json_type(e.payload_json,'$.triggering_credit_id')='integer' GROUP BY l.id) f ON f.id=json_extract(e.payload_json,'$.triggering_credit_id') WHERE e.event_type='feeder_confirmed' AND o.status IN ('pending','failed') AND e.seq!=f.first_seq ORDER BY o.event_id LIMIT 100001").fetch_all(old).await?;
    let actual: Vec<String> = sqlx::query_scalar(
        "SELECT event_id FROM message_outbox_suppressions ORDER BY event_id LIMIT 100001",
    )
    .fetch_all(new)
    .await?;
    let invalid: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM message_outbox_suppressions WHERE reason!='duplicate_feeding_announcement' OR created_at<0 OR created_at>unixepoch()").fetch_one(new).await?;
    if expected.len() > 100000 || actual != expected || invalid != 0 {
        bail!("duplicate-message suppression backfill changed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn feeder_cap_migration_preserves_paid_rows_and_refuses_changes_and_activation() {
        let root = tempfile::TempDir::new().unwrap();
        let (p, _keep) = super::super::contact_upgrade::tests::seed(root.path()).await;
        let source = format!("sqlite://{}", p.components[&Role::Core].database.display());
        let pool = crate::sqlite::connect_durable(&source, 1).await.unwrap();
        let mut prefix = sqlx::migrate!("./migrations");
        prefix.migrations = prefix
            .iter()
            .filter(|m| m.version <= 24)
            .cloned()
            .collect::<Vec<_>>()
            .into();
        prefix.run(&pool).await.unwrap();
        pool.close().await;
        let before = root.path().join("before");
        let after = root.path().join("after");
        super::super::contact_upgrade::tests::copy_images(&p, &before);
        super::super::contact_upgrade::tests::copy_images(&p, &after);
        database::inspect_before_feeder_cap(&before, &p)
            .await
            .unwrap();
        assert!(database::inspect(&before, &p).await.is_err());
        let pool = crate::sqlite::connect_durable(
            &format!("sqlite://{}", after.join("core.db").display()),
            1,
        )
        .await
        .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        compare_images(&before, &after, &p).await.unwrap();
        sqlx::query("UPDATE ledger_entries SET delta_sats=delta_sats+1 WHERE id=1")
            .execute(&pool)
            .await
            .unwrap();
        assert!(compare_images(&before, &after, &p).await.is_err());
        sqlx::query("UPDATE ledger_entries SET delta_sats=delta_sats-1 WHERE id=1")
            .execute(&pool)
            .await
            .unwrap();
        compare_images(&before, &after, &p).await.unwrap();
        sqlx::query("INSERT INTO feeder_credit_policy VALUES(1,1000,3)")
            .execute(&pool)
            .await
            .unwrap();
        assert!(compare_images(&before, &after, &p).await.is_err());
        sqlx::query("DELETE FROM feeder_credit_policy")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE TRIGGER unqualified_feeder_trigger AFTER INSERT ON feeder_credit_policy BEGIN SELECT 1; END").execute(&pool).await.unwrap();
        assert!(compare_images(&before, &after, &p).await.is_err());
        sqlx::query("DROP TRIGGER unqualified_feeder_trigger")
            .execute(&pool)
            .await
            .unwrap();
        compare_images(&before, &after, &p).await.unwrap();
        pool.close().await;
    }
}
