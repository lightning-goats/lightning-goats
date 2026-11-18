use super::{Policy, Role};
use anyhow::{Result, bail};
use futures_util::TryStreamExt as _;
use serde::{Deserialize, Serialize};
use sqlx::{
    Row as _, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{collections::BTreeMap, ops::RangeInclusive, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub engine: String,
    pub rows: BTreeMap<Role, BTreeMap<String, u64>>,
    pub pending_feeds: u64,
    pub unresolved_provider_payments: u64,
    pub execution_claims: u64,
    pub payout_held_sats: u64,
    pub retention: String,
    pub resume_authorized: bool,
}

static CORE: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");
static MEMBERSHIP: sqlx::migrate::Migrator = sqlx::migrate!("./cyberherd-migrations");
static PAYOUT: sqlx::migrate::Migrator = sqlx::migrate!("./payout-migrations");
static SIGNER: sqlx::migrate::Migrator = sqlx::migrate!("./signer-migrations");
pub(super) const CURRENT_CORE_SCHEMA: i64 = 27;

#[derive(Clone, Copy)]
enum ReviewScope {
    OrdinaryMaintenance,
    CashuCorrelationsOnly,
}

pub(super) async fn inspect(directory: &Path, policy: &Policy) -> Result<Report> {
    inspect_version(
        directory,
        policy,
        CURRENT_CORE_SCHEMA,
        false,
        ReviewScope::OrdinaryMaintenance,
    )
    .await
}

/// Only the distinct, read-only Cashu correlation review can select this scope.
/// It never produces an ordinary accepted backup or restore authority.
pub(super) async fn inspect_cashu_correlations(
    directory: &Path,
    policy: &Policy,
) -> Result<Report> {
    Ok(inspect_versions(
        directory,
        policy,
        26..=CURRENT_CORE_SCHEMA,
        false,
        ReviewScope::CashuCorrelationsOnly,
    )
    .await?
    .0)
}
pub(super) async fn inspect_schema_25(directory: &Path, policy: &Policy) -> Result<Report> {
    inspect_version(
        directory,
        policy,
        25,
        false,
        ReviewScope::OrdinaryMaintenance,
    )
    .await
}
#[cfg(test)]
pub(super) async fn migrate_core_through(pool: &SqlitePool, version: i64) -> Result<()> {
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.migrations = migrator
        .iter()
        .filter(|m| m.version <= version)
        .cloned()
        .collect::<Vec<_>>()
        .into();
    migrator.run(pool).await?;
    Ok(())
}
pub(super) async fn inspect_before_contact(directory: &Path, policy: &Policy) -> Result<Report> {
    inspect_version(
        directory,
        policy,
        23,
        false,
        ReviewScope::OrdinaryMaintenance,
    )
    .await
}
pub(super) async fn inspect_before_feeder_cap(directory: &Path, policy: &Policy) -> Result<Report> {
    inspect_version(
        directory,
        policy,
        24,
        false,
        ReviewScope::OrdinaryMaintenance,
    )
    .await
}
pub(super) async fn inspect_before_inclusive_fees(
    directory: &Path,
    policy: &Policy,
) -> Result<Report> {
    inspect_inclusive_fees(directory, policy, true).await
}

pub(super) async fn inspect_after_inclusive_fees(
    directory: &Path,
    policy: &Policy,
) -> Result<Report> {
    inspect_inclusive_fees(directory, policy, false).await
}

async fn inspect_inclusive_fees(
    directory: &Path,
    policy: &Policy,
    before_fees: bool,
) -> Result<Report> {
    Ok(inspect_upgrade_copy(directory, policy, 25, before_fees)
        .await?
        .0)
}

pub(super) async fn inspect_upgrade_copy(
    directory: &Path,
    policy: &Policy,
    oldest: i64,
    before_fees: bool,
) -> Result<(Report, i64)> {
    let (report, version) = inspect_versions(
        directory,
        policy,
        oldest..=CURRENT_CORE_SCHEMA,
        before_fees,
        ReviewScope::OrdinaryMaintenance,
    )
    .await?;
    if version >= 26 {
        for table in super::cashu_upgrade::tables_for_core(version) {
            if report.rows[&Role::Core].get(table) != Some(&0) {
                bail!("maintenance requires empty inactive Cashu tables");
            }
        }
    }
    Ok((report, version))
}
async fn inspect_version(
    directory: &Path,
    policy: &Policy,
    core_version: i64,
    before_fees: bool,
    scope: ReviewScope,
) -> Result<Report> {
    Ok(inspect_versions(
        directory,
        policy,
        core_version..=core_version,
        before_fees,
        scope,
    )
    .await?
    .0)
}

async fn inspect_versions(
    directory: &Path,
    policy: &Policy,
    versions: RangeInclusive<i64>,
    before_fees: bool,
    scope: ReviewScope,
) -> Result<(Report, i64)> {
    let mut pools = BTreeMap::new();
    let mut rows = BTreeMap::new();
    let mut core_version = *versions.end();
    for role in Role::ALL {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(directory.join(role.file()))
                    .read_only(true)
                    // Only a complete main-only copy is immutable. That avoids
                    // an invented empty WAL without ignoring real copied WAL.
                    .immutable(
                        !directory
                            .join(format!("{}-wal", role.file()))
                            .try_exists()?,
                    )
                    .create_if_missing(false)
                    .foreign_keys(true)
                    .pragma("query_only", "ON")
                    .pragma("trusted_schema", "OFF")
                    .pragma("hard_heap_limit", "268435456")
                    .busy_timeout(std::time::Duration::from_secs(5)),
            )
            .await?;
        let engine: String = sqlx::query_scalar("SELECT sqlite_version()")
            .fetch_one(&pool)
            .await?;
        if engine != "3.54.1" {
            bail!("state operations require the qualified patched SQLite engine");
        }
        let integrity: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_all(&pool)
            .await?;
        if integrity != ["ok"]
            || sqlx::query("PRAGMA foreign_key_check")
                .fetch_optional(&pool)
                .await?
                .is_some()
        {
            bail!("journal integrity or foreign keys failed");
        }
        if role == Role::Core && versions.start() != versions.end() {
            // Select a supported schema before running its exact migration and
            // journal checks. A correlation failure must retain its cause.
            core_version = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
                .fetch_one(&pool)
                .await?;
            if !versions.contains(&core_version) {
                bail!("maintenance requires a supported core schema");
            }
        }
        if core_version != CURRENT_CORE_SCHEMA && role == Role::Core {
            require_core_prefix(&pool, core_version).await?;
        } else if before_fees && role == Role::Payout {
            require_before_inclusive_fees(&pool).await?;
        } else {
            crate::sqlite::require_current_schema(
                &pool,
                match role {
                    Role::Core => &CORE,
                    Role::Membership => &MEMBERSHIP,
                    Role::Payout => &PAYOUT,
                    Role::Signer => &SIGNER,
                },
            )
            .await?;
        }
        let tables:Vec<String>=sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name LIMIT 257").fetch_all(&pool).await?;
        if tables.len() > 256 {
            bail!("journal schema exceeds maintenance bound");
        }
        let mut counts = BTreeMap::new();
        for table in tables {
            if table.is_empty()
                || table.len() > 128
                || !table
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_')
            {
                bail!("unsupported journal table identifier");
            }
            let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM \"{table}\" "))
                .fetch_one(&pool)
                .await?;
            counts.insert(table, u64::try_from(count)?);
        }
        rows.insert(role, counts);
        pools.insert(role, pool);
    }
    let report = report(&pools, policy, rows, core_version, scope).await;
    for pool in pools.into_values() {
        pool.close().await;
    }
    Ok((report?, core_version))
}

async fn report(
    pools: &BTreeMap<Role, SqlitePool>,
    policy: &Policy,
    rows: BTreeMap<Role, BTreeMap<String, u64>>,
    core_version: i64,
    scope: ReviewScope,
) -> Result<Report> {
    cross_check(pools, policy).await?;
    let core = &pools[&Role::Core];
    if core_version >= 24 {
        crate::contact_state::verify(core).await?;
    }
    if core_version >= 26 {
        let cashu: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cashu_receipt_inbox)")
            .fetch_one(core)
            .await?;
        if cashu && matches!(scope, ReviewScope::OrdinaryMaintenance) {
            bail!(
                "Cashu custody requires a coordinated wallet/core backup; four-journal maintenance is insufficient"
            );
        }
    }
    let payout = &pools[&Role::Payout];
    Ok(Report {
        engine: "3.54.1".into(),
        pending_feeds: count(core, "SELECT COUNT(*) FROM feed_attempts WHERE status IN ('intent_committed','unknown')").await?,
        unresolved_provider_payments: count(payout, "SELECT COUNT(*) FROM payout_execute_intents e LEFT JOIN payout_payment_bindings b ON b.intent_id=e.intent_id WHERE b.payment_id IS NULL OR EXISTS(SELECT 1 FROM payout_payment_conflicts c WHERE c.payment_id=b.payment_id) OR NOT EXISTS(SELECT 1 FROM payout_payment_records r WHERE r.payment_id=b.payment_id AND r.state IN ('COMPLETED','FAILED'))").await?,
        execution_claims: count(core, "SELECT COUNT(*) FROM payout_execution_claims").await?,
        payout_held_sats: count(payout, "SELECT COALESCE((SELECT SUM(g.reserved_sats) FROM payout_grants g WHERE NOT EXISTS(SELECT 1 FROM payout_outcome_acks a WHERE a.grant_id=g.id)),0)+COALESCE((SELECT SUM(held_sats) FROM payout_outcome_acks),0)-COALESCE((SELECT SUM(gross_sats) FROM payout_cost_acks),0)").await?,
        rows,
        retention: "retain_all_financial_replay_and_signed_history".into(),
        resume_authorized: false,
    })
}
// This explicit additive-upgrade boundary never changes ordinary backup or
// restore verification: only the separate contact comparison can select it.
async fn require_core_prefix(pool: &SqlitePool, version: i64) -> Result<()> {
    let migrations: Vec<_> = CORE.iter().collect();
    if !matches!(version, 23..=26)
        || migrations.len() != usize::try_from(CURRENT_CORE_SCHEMA)?
        || migrations.last().map(|m| m.version) != Some(CURRENT_CORE_SCHEMA)
    {
        bail!("additive upgrade requires the separately qualified schema prefix");
    }
    let rows = sqlx::query(
        "SELECT version,success,checksum FROM _sqlx_migrations ORDER BY version LIMIT 28",
    )
    .fetch_all(pool)
    .await?;
    if rows.len() != usize::try_from(version)? {
        bail!("additive upgrade source has the wrong schema version");
    }
    for (row, migration) in rows.iter().zip(migrations) {
        if row.try_get::<i64, _>("version")? != migration.version
            || !row.try_get::<bool, _>("success")?
            || row.try_get::<Vec<u8>, _>("checksum")?.as_slice() != migration.checksum.as_ref()
        {
            bail!("additive upgrade source has unknown, dirty or changed migrations");
        }
    }
    if version == 23 {
        let private: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name LIKE 'private_contact_%')",
        )
        .fetch_one(pool)
        .await?;
        if private {
            bail!("contact upgrade source already contains private-contact schema");
        }
    }
    let later: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE
            (?<26 AND (name LIKE 'cashu_%' OR name LIKE 'feed_source_%'))
            OR tbl_name='cashu_treasury_fee_observations')",
    )
    .bind(version)
    .fetch_one(pool)
    .await?;
    if later {
        bail!("Cashu upgrade source already contains new schema objects");
    }
    Ok(())
}

async fn count(pool: &SqlitePool, query: &str) -> Result<u64> {
    Ok(u64::try_from(
        sqlx::query_scalar::<_, i64>(query).fetch_one(pool).await?,
    )?)
}
async fn bounded_json(pool: &SqlitePool, query: &str, key: Option<&str>) -> Result<String> {
    let q = sqlx::query_scalar::<_, String>(query);
    let value = if let Some(key) = key {
        q.bind(key).fetch_one(pool).await?
    } else {
        q.fetch_one(pool).await?
    };
    if value.len() > 131072 {
        bail!("journal correlation exceeds bound");
    }
    Ok(value)
}
// Fee discovery uses independent pre-execution IDs. Some are never promoted;
// promoted IDs exist in both tables and must retain byte-identical documents.
async fn has_probe_table(pool: &SqlitePool, name: &str) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)",
    )
    .bind(name)
    .fetch_one(pool)
    .await?)
}
async fn signing_intent(pool: &SqlitePool, id: &str) -> Result<String> {
    let mut documents: Vec<Option<String>> = Vec::new();
    documents.extend(sqlx::query_scalar::<_,Option<String>>("SELECT CASE WHEN length(CAST(document_json AS BLOB))<=131072 THEN document_json END FROM payout_invoice_intents WHERE id=?")
        .bind(id).fetch_all(pool).await?);
    if has_probe_table(pool, "payout_fee_probe_intents").await? {
        documents.extend(sqlx::query_scalar::<_,Option<String>>("SELECT CASE WHEN length(CAST(document_json AS BLOB))<=131072 THEN document_json END FROM payout_fee_probe_intents WHERE id=?")
            .bind(id).fetch_all(pool).await?);
    }
    let first = documents.first().and_then(|v| v.as_ref()).ok_or_else(|| {
        anyhow::anyhow!("signer has no bounded original payout or fee-probe intent")
    })?;
    if documents.iter().any(|v| v.as_ref() != Some(first)) {
        bail!("promoted fee-probe signing intent differs from original");
    }
    Ok(first.clone())
}
async fn signing_callback(pool: &SqlitePool, id: &str, signed: &str) -> Result<()> {
    let mut callbacks: Vec<Option<String>> =
        sqlx::query_scalar("SELECT raw_zap FROM payout_callback_requests WHERE intent_id=?")
            .bind(id)
            .fetch_all(pool)
            .await?;
    if has_probe_table(pool, "payout_fee_probe_callbacks").await? {
        callbacks.extend(
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT raw_zap FROM payout_fee_probe_callbacks WHERE intent_id=?",
            )
            .bind(id)
            .fetch_all(pool)
            .await?,
        );
    }
    if let Some(first) = callbacks.first() {
        if callbacks.iter().any(|v| v != first) {
            bail!("promoted fee-probe callback differs from original");
        }
        // A valid but unused signature can survive a lost response followed by
        // the operator-approved ordinary callback. It is never payment proof.
        if first.as_deref().is_some_and(|raw| raw != signed) {
            bail!("signer result differs from frozen payout callback bytes");
        }
    }
    Ok(())
}
async fn cross_check(pools: &BTreeMap<Role, SqlitePool>, policy: &Policy) -> Result<()> {
    let core = &pools[&Role::Core];
    let membership = &pools[&Role::Membership];
    let payout = &pools[&Role::Payout];
    let signer = &pools[&Role::Signer];
    let epoch: String =
        sqlx::query_scalar("SELECT stream_id FROM overlay_identity WHERE singleton=1")
            .fetch_one(core)
            .await?;
    if epoch != policy.epoch.to_string() {
        bail!("core epoch differs from the protected maintenance policy");
    }
    let core_max: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(seq),0) FROM event_log")
        .fetch_one(core)
        .await?;
    let selected: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM owners WHERE owner=?")
        .bind(&policy.owner)
        .fetch_one(membership)
        .await?;
    if selected != 1 {
        bail!("selected membership owner is absent");
    }
    let mut bindings = sqlx::query("SELECT owner,binding_json,received_through FROM core_streams")
        .fetch(membership);
    let mut found = false;
    while let Some(row) = bindings.try_next().await? {
        let raw: String = row.try_get("binding_json")?;
        if raw.len() > 16384 {
            bail!("core binding exceeds bound");
        }
        let binding: crate::cyberherd::ingress::CoreBinding = serde_json::from_str(&raw)?;
        binding.validate()?;
        let through: i64 = row.try_get("received_through")?;
        if binding.core_uid != policy.components[&Role::Core].uid
            || binding.epoch != policy.epoch
            || through < 0
            || (through as u64) < binding.start_after
            || through > core_max
        {
            bail!("membership cursor or epoch does not belong to this core image");
        }
        if row.try_get::<String, _>("owner")? == policy.owner {
            found = true;
        }
    }
    drop(bindings);
    if !found {
        bail!("selected owner's core activation binding is absent");
    }
    let scope: crate::payout::store::Scope = serde_json::from_str(
        &bounded_json(
            payout,
            "SELECT scope_json FROM payout_binding WHERE singleton=1",
            None,
        )
        .await?,
    )?;
    if scope.core_uid != policy.components[&Role::Core].uid
        || scope.epoch != policy.epoch
        || scope.owner != policy.owner
        || scope.provider != policy.provider
    {
        bail!("payout scope differs from the core and protected provider binding");
    }
    let signer_binding = sqlx::query("SELECT owner,peer_uid FROM signer_binding WHERE singleton=1")
        .fetch_one(signer)
        .await?;
    if signer_binding.try_get::<String, _>("owner")? != policy.owner
        || signer_binding.try_get::<i64, _>("peer_uid")?
            != i64::from(policy.components[&Role::Payout].uid)
    {
        bail!("signer does not belong to the selected payout identity");
    }

    let mut grants = sqlx::query("SELECT id,request_id,plan_hash,reserved_sats,CASE WHEN length(CAST(plan_json AS BLOB))<=4194304 THEN plan_json END AS plan_json FROM payout_grants")
        .fetch(payout);
    while let Some(row) = grants.try_next().await? {
        let id: String = row.try_get("id")?;
        let original=sqlx::query("SELECT request_id,plan_hash,reserved_sats,epoch,owner FROM reward_reservations WHERE id=?").bind(&id).fetch_one(core).await?;
        if row.try_get::<String, _>("request_id")? != original.try_get::<String, _>("request_id")?
            || row.try_get::<String, _>("plan_hash")?
                != original.try_get::<String, _>("plan_hash")?
            || row.try_get::<i64, _>("reserved_sats")?
                != original.try_get::<i64, _>("reserved_sats")?
            || original.try_get::<String, _>("epoch")? != policy.epoch.to_string()
            || original.try_get::<String, _>("owner")? != policy.owner
        {
            bail!("payout grant lost its original core reservation");
        }
        let frozen =
            sqlx::query("SELECT plan_hash,CASE WHEN length(CAST(plan_json AS BLOB))<=4194304 THEN plan_json END AS plan_json FROM reward_plans WHERE owner=? AND request_id=?")
                .bind(&policy.owner)
                .bind(row.try_get::<String, _>("request_id")?)
                .fetch_one(membership)
                .await?;
        let payout_plan: String = row.try_get("plan_json")?;
        let frozen_plan: String = frozen.try_get("plan_json")?;
        let plan: crate::cyberherd::rewards::Plan = serde_json::from_str(&frozen_plan)?;
        if frozen.try_get::<String, _>("plan_hash")? != row.try_get::<String, _>("plan_hash")?
            || payout_plan != frozen_plan
            || plan.hash()? != frozen.try_get::<String, _>("plan_hash")?
            || plan.owner != policy.owner
            || plan.epoch != policy.epoch
            || plan.core_uid != policy.components[&Role::Core].uid
            || plan.request_id.to_string() != row.try_get::<String, _>("request_id")?
            || i64::try_from(plan.reserved_sats)? != row.try_get::<i64, _>("reserved_sats")?
        {
            bail!("payout grant lost its frozen membership plan");
        }
    }
    drop(grants);
    let mut claims = sqlx::query("SELECT document_json FROM payout_execution_claims").fetch(core);
    while let Some(row) = claims.try_next().await? {
        let raw: String = row.try_get("document_json")?;
        if raw.len() > 32768 {
            bail!("execution claim exceeds bound");
        }
        let record: crate::payout::execution::Record = serde_json::from_str(&raw)?;
        record.validate()?;
        let quote = record.request.quote()?;
        if record.request.reservation.epoch != policy.epoch
            || record.request.reservation.owner != policy.owner
            || quote.request.scope != policy.provider
        {
            bail!("execution claim has a different root scope");
        }
        let local = bounded_json(
            payout,
            "SELECT document_json FROM payout_quote_records WHERE intent_id=?",
            Some(&quote.request.id.to_string()),
        )
        .await?;
        if local != record.request.quote_json {
            bail!("payout image lacks the original claimed quote");
        }
    }
    drop(claims);
    equal_rows(payout,core,"SELECT grant_id AS key,document_json AS body FROM payout_core_execution_claims","SELECT document_json FROM payout_execution_claims WHERE reservation_id=? AND document_json=?").await?;
    equal_rows(payout,core,"SELECT grant_id AS key,progress_json AS body FROM payout_outcome_acks","SELECT progress_json FROM reward_outcome_finals WHERE reservation_id=? AND progress_json=?").await?;
    equal_rows(
        payout,
        core,
        "SELECT grant_id AS key,document_json AS body FROM payout_cost_acks",
        "SELECT document_json FROM payout_failed_costs WHERE reservation_id=? AND document_json=?",
    )
    .await?;

    let mut intents = sqlx::query("SELECT id,document_json FROM signer_intents").fetch(signer);
    while let Some(row) = intents.try_next().await? {
        let id: String = row.try_get("id")?;
        let original = signing_intent(payout, &id).await?;
        if row.try_get::<String, _>("document_json")? != original {
            bail!("signer lost its original payout invoice intent");
        }
    }
    drop(intents);
    // Validate signed broker results without rewriting them or assuming that a
    // callback/result ACK has already reached the other journal.
    let mut results = sqlx::query("SELECT id,event_json FROM signer_results").fetch(signer);
    while let Some(row) = results.try_next().await? {
        let id: String = row.try_get("id")?;
        let document = signing_intent(payout, &id).await?;
        let intent = crate::payout::invoice::Intent::restore(serde_json::from_str(&document)?)?;
        let signed: String = row.try_get("event_json")?;
        crate::payout::invoice::Request::restore(intent, Some(signed.clone()))?;
        signing_callback(payout, &id, &signed).await?;
    }
    drop(results);
    let _ = super::reward_cutover::validate_recorded(core, membership, policy).await?;
    Ok(())
}
async fn equal_rows(local: &SqlitePool, core: &SqlitePool, rows: &str, lookup: &str) -> Result<()> {
    let mut stream = sqlx::query(rows).fetch(local);
    while let Some(row) = stream.try_next().await? {
        let key: String = row.try_get("key")?;
        let body: String = row.try_get("body")?;
        if body.len() > 131072 {
            bail!("cross-journal receipt exceeds bound");
        }
        let original: Option<String> = sqlx::query_scalar(lookup)
            .bind(key)
            .bind(&body)
            .fetch_optional(core)
            .await?;
        if original.as_deref() != Some(&body) {
            bail!("payout acknowledgement lost its exact core receipt");
        }
    }
    Ok(())
}

pub(super) async fn require_before_inclusive_fees(pool: &SqlitePool) -> Result<()> {
    let compiled: Vec<_> = PAYOUT.iter().collect();
    if compiled.iter().map(|m| m.version).collect::<Vec<_>>() != [1, 2, 3, 4, 5, 6, 8, 9, 10, 11] {
        bail!("inclusive-fee source requires the separately qualified payout migrations");
    }
    let expected: Vec<_> = compiled.into_iter().filter(|m| m.version <= 8).collect();
    let rows = sqlx::query(
        "SELECT version,success,checksum FROM _sqlx_migrations ORDER BY version LIMIT 12",
    )
    .fetch_all(pool)
    .await?;
    if rows.len() != expected.len() {
        bail!("inclusive-fee source must be exactly payout schema 8");
    }
    for (row, migration) in rows.iter().zip(expected) {
        if row.try_get::<i64, _>("version")? != migration.version
            || !row.try_get::<bool, _>("success")?
            || row.try_get::<Vec<u8>, _>("checksum")?.as_slice() != migration.checksum.as_ref()
        {
            bail!("inclusive-fee source migration history changed");
        }
    }
    let extra: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name LIKE 'payout_fee_%')",
    )
    .fetch_one(pool)
    .await?;
    if extra {
        bail!("inclusive-fee source already contains fee schema objects");
    }
    Ok(())
}
#[cfg(test)]
mod inclusive_fee_prefix_tests {
    use super::*;
    #[tokio::test]
    async fn exact_payout_prefix_preserves_version_gap_and_rejects_changed_or_current_history() {
        let root = tempfile::TempDir::new().unwrap();
        let pool = crate::sqlite::connect_durable(
            &format!("sqlite://{}", root.path().join("payout.db").display()),
            1,
        )
        .await
        .unwrap();
        let mut old = sqlx::migrate!("./payout-migrations");
        old.migrations = old
            .iter()
            .filter(|m| m.version <= 8)
            .cloned()
            .collect::<Vec<_>>()
            .into();
        old.run(&pool).await.unwrap();
        require_before_inclusive_fees(&pool).await.unwrap();
        sqlx::query("UPDATE _sqlx_migrations SET success=0 WHERE version=8")
            .execute(&pool)
            .await
            .unwrap();
        assert!(require_before_inclusive_fees(&pool).await.is_err());
        sqlx::query("UPDATE _sqlx_migrations SET success=1 WHERE version=8")
            .execute(&pool)
            .await
            .unwrap();
        PAYOUT.run(&pool).await.unwrap();
        assert!(require_before_inclusive_fees(&pool).await.is_err());
        pool.close().await;
    }
}

#[cfg(test)]
mod probe_correlation_tests {
    use super::*;
    #[tokio::test]
    async fn unpromoted_promoted_and_legacy_signing_records_keep_strict_correlations() {
        let directory = tempfile::tempdir().unwrap();
        let url = format!(
            "sqlite://{}",
            directory.path().join("correlation.db").display()
        );
        let pool = crate::sqlite::connect_durable(&url, 1).await.unwrap();
        for ddl in [
            "CREATE TABLE payout_invoice_intents(id TEXT PRIMARY KEY,document_json TEXT)",
            "CREATE TABLE payout_callback_requests(intent_id TEXT PRIMARY KEY,raw_zap TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO payout_invoice_intents VALUES('legacy','exact')")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(signing_intent(&pool, "legacy").await.unwrap(), "exact");
        signing_callback(&pool, "legacy", "signed").await.unwrap();
        assert!(signing_intent(&pool, "missing").await.is_err());
        for ddl in [
            "CREATE TABLE payout_fee_probe_intents(id TEXT PRIMARY KEY,document_json TEXT)",
            "CREATE TABLE payout_fee_probe_callbacks(intent_id TEXT PRIMARY KEY,raw_zap TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO payout_fee_probe_intents VALUES('probe','original')")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(signing_intent(&pool, "probe").await.unwrap(), "original");
        sqlx::query("INSERT INTO payout_fee_probe_callbacks VALUES('probe','signed')")
            .execute(&pool)
            .await
            .unwrap();
        signing_callback(&pool, "probe", "signed").await.unwrap();
        assert!(signing_callback(&pool, "probe", "changed").await.is_err());
        sqlx::query("INSERT INTO payout_invoice_intents VALUES('probe','original')")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(signing_intent(&pool, "probe").await.unwrap(), "original");
        sqlx::query("UPDATE payout_invoice_intents SET document_json='changed' WHERE id='probe'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(signing_intent(&pool, "probe").await.is_err());
        sqlx::query("INSERT INTO payout_callback_requests VALUES('probe',NULL)")
            .execute(&pool)
            .await
            .unwrap();
        assert!(signing_callback(&pool, "probe", "signed").await.is_err());
        sqlx::query("UPDATE payout_fee_probe_callbacks SET raw_zap=NULL WHERE intent_id='probe'")
            .execute(&pool)
            .await
            .unwrap();
        signing_callback(&pool, "probe", "signed").await.unwrap();
        sqlx::query("INSERT INTO payout_fee_probe_intents VALUES('oversized',?)")
            .bind("x".repeat(131073))
            .execute(&pool)
            .await
            .unwrap();
        assert!(signing_intent(&pool, "oversized").await.is_err());
        pool.close().await;
    }
}
