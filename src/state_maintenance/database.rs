use super::{Policy, Role};
use anyhow::{Result, bail};
use futures_util::TryStreamExt as _;
use serde::{Deserialize, Serialize};
use sqlx::{
    Row as _, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{collections::BTreeMap, path::Path};

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

pub(super) async fn inspect(directory: &Path, policy: &Policy) -> Result<Report> {
    inspect_version(directory, policy, 25).await
}
pub(super) async fn inspect_before_contact(directory: &Path, policy: &Policy) -> Result<Report> {
    inspect_version(directory, policy, 23).await
}
pub(super) async fn inspect_before_feeder_cap(directory: &Path, policy: &Policy) -> Result<Report> {
    inspect_version(directory, policy, 24).await
}
async fn inspect_version(directory: &Path, policy: &Policy, core_version: i64) -> Result<Report> {
    let mut pools = BTreeMap::new();
    let mut rows = BTreeMap::new();
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
        if core_version != 25 && role == Role::Core {
            require_core_prefix(&pool, core_version).await?;
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
    let report = report(&pools, policy, rows, core_version).await;
    for pool in pools.into_values() {
        pool.close().await;
    }
    report
}

async fn report(
    pools: &BTreeMap<Role, SqlitePool>,
    policy: &Policy,
    rows: BTreeMap<Role, BTreeMap<String, u64>>,
    core_version: i64,
) -> Result<Report> {
    cross_check(pools, policy).await?;
    let core = &pools[&Role::Core];
    if core_version >= 24 {
        crate::contact_state::verify(core).await?;
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
    if !matches!(version, 23 | 24)
        || migrations.len() != 25
        || migrations.last().map(|m| m.version) != Some(25)
    {
        bail!("additive upgrade requires the separately qualified schema prefix");
    }
    let rows = sqlx::query(
        "SELECT version,success,checksum FROM _sqlx_migrations ORDER BY version LIMIT 26",
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
        let original = bounded_json(
            payout,
            "SELECT document_json FROM payout_invoice_intents WHERE id=?",
            Some(&id),
        )
        .await?;
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
        let document = bounded_json(
            payout,
            "SELECT document_json FROM payout_invoice_intents WHERE id=?",
            Some(&id),
        )
        .await?;
        let intent = crate::payout::invoice::Intent::restore(serde_json::from_str(&document)?)?;
        let signed: String = row.try_get("event_json")?;
        crate::payout::invoice::Request::restore(intent, Some(signed.clone()))?;
        let frozen: Option<Option<String>> =
            sqlx::query_scalar("SELECT raw_zap FROM payout_callback_requests WHERE intent_id=?")
                .bind(&id)
                .fetch_optional(payout)
                .await?;
        if frozen.is_some_and(|raw| raw.as_deref() != Some(&signed)) {
            bail!("signer result differs from frozen payout callback bytes");
        }
    }
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
