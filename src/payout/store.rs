//! Independent, durable payout funding admission. This store owns neither the
//! core nor membership DB and grants no provider execution or publication API.
use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool, migrate::Migrator};
use uuid::Uuid;

use super::{
    funding::{ProviderScope, QualifiedFunding},
    recipient::address_identity,
};
use crate::cyberherd::rewards::Plan;

static MIGRATOR: Migrator = sqlx::migrate!("./payout-migrations");

#[path = "store_failed_costs.rs"]
mod failed_costs;
#[path = "store_fee_probes.rs"]
mod fee_probes;
#[path = "store_invoices.rs"]
mod invoices;
#[path = "store_outcomes.rs"]
mod outcomes;
#[path = "store_provider.rs"]
mod provider;
#[cfg(unix)]
#[path = "store_worker.rs"]
mod worker;
pub use provider::Status as PayoutStatus;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub core_uid: u32,
    pub epoch: Uuid,
    pub owner: String,
    pub provider: ProviderScope,
}

impl Scope {
    fn validate(&self) -> Result<()> {
        crate::nostr_event::validate_public_key(&self.owner)?;
        crate::nostr_event::validate_hex(&self.provider.credential_fingerprint, 32)?;
        if self.core_uid == 0
            || self.epoch.is_nil()
            || self.provider.environment != "strike-production"
            || self.provider.account_id.is_nil()
        {
            bail!("invalid root payout scope");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub scope: Scope,
    pub member_percent_ceiling: u8,
    pub max_sats_per_feed: u64,
    pub max_sats_per_24h: u64,
    pub max_fee_sats: u64,
    #[serde(default, skip_serializing_if = "super::inclusive_fees::Mode::is_fixed")]
    pub fee_mode: super::inclusive_fees::Mode,
    /// Bounds both ordinary Lightning and NIP-57 independently of signing.
    #[serde(default)]
    pub max_net_sats: u64,
    pub max_source_parts: u16,
    pub operation_addresses: Vec<String>,
    pub receipt_relays: Vec<String>,
    #[serde(default)]
    pub execution_enabled: bool,
}

impl Policy {
    pub fn validate(&self) -> Result<()> {
        self.scope.validate()?;
        if (!self.fee_mode.is_fixed() && self.max_fee_sats != 0)
            || self.member_percent_ceiling > 100
            || self.max_sats_per_feed > i64::MAX as u64
            || self.max_sats_per_24h > i64::MAX as u64
            || self.max_fee_sats > i64::MAX as u64
            || self.max_net_sats > i64::MAX as u64 / 1000
            || !(1..=8192).contains(&self.max_source_parts)
            || self.operation_addresses.len() > 128
        {
            bail!("invalid independent payout policy");
        }
        for address in &self.operation_addresses {
            address_identity(address)?;
        }
        super::invoice::receipt_relays(&self.receipt_relays)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Grant {
    pub id: Uuid,
    pub request_id: Uuid,
    pub plan_hash: String,
    pub reserved_sats: u64,
    pub admitted_at: i64,
}

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
    policy: Policy,
}

impl Store {
    pub(crate) async fn close_after_initialization(self) {
        self.pool.close().await;
    }
    /// Existing, disabled schema-8 journal only. This opens no worker, signer,
    /// provider or core connection and never creates a scope/intake binding.
    #[cfg(unix)]
    pub(crate) async fn migrate_fee_schema_offline(
        database_url: &str,
        policy: &Policy,
        intake_uid: u32,
    ) -> Result<()> {
        policy.validate()?;
        if policy.execution_enabled
            || policy.member_percent_ceiling != 0
            || policy.max_sats_per_feed != 0
            || policy.max_sats_per_24h != 0
            || policy.max_fee_sats != 0
            || policy.max_net_sats != 0
        {
            bail!("offline fee migration requires zero financial authority");
        }
        super::cost_recovery::existing_private_database(database_url)?;
        let pool = crate::sqlite::connect_existing_durable(database_url, 1).await?;
        let result = async {
            crate::state_maintenance::require_payout_fee_upgrade_source(&pool).await?;
            let saved: String =
                sqlx::query_scalar("SELECT scope_json FROM payout_binding WHERE singleton=1")
                    .fetch_one(&pool)
                    .await?;
            let peer: i64 =
                sqlx::query_scalar("SELECT peer_uid FROM payout_intake_binding WHERE singleton=1")
                    .fetch_one(&pool)
                    .await?;
            if saved != serde_json::to_string(&policy.scope)? || peer != i64::from(intake_uid) {
                bail!("offline fee migration scope or intake identity changed");
            }
            MIGRATOR.run(&pool).await?;
            crate::sqlite::require_current_schema(&pool, &MIGRATOR).await
        }
        .await;
        pool.close().await;
        result
    }
    pub async fn connect(database_url: &str, policy: Policy) -> Result<Self> {
        policy.validate()?;
        let pool = crate::sqlite::connect_durable(database_url, 5).await?;
        // Never create payout tables inside an accidentally selected core or
        // membership database, even if its migration version happens to match.
        let names: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
        )
        .fetch_all(&pool)
        .await?;
        let allowed = [
            "_sqlx_migrations",
            "payout_binding",
            "payout_clock",
            "payout_grants",
            "payout_sources",
            "payout_source_parts",
            "payout_rows",
            "payout_journal",
            "payout_invoice_intents",
            "payout_callback_requests",
            "payout_invoices",
            "payout_invoice_journal",
            "payout_quote_intents",
            "payout_fee_observations",
            "payout_fee_probe_intents",
            "payout_fee_probe_callbacks",
            "payout_fee_probe_invoices",
            "payout_fee_probe_quotes",
            "payout_fee_promotions",
            "payout_fee_probe_duplicates",
            "payout_quote_records",
            "payout_quote_duplicates",
            "payout_execute_intents",
            "payout_payment_bindings",
            "payout_payment_records",
            "payout_payment_conflicts",
            "payout_provider_journal",
            "payout_unsubmitted",
            "payout_outcome_manifests",
            "payout_outcome_acks",
            "payout_intake_binding",
            "payout_intake_receipts",
            "payout_worker_clock",
            "payout_worker_attempts",
            "payout_core_execution_claims",
            "payout_cost_acks",
        ];
        if names.iter().any(|name| !allowed.contains(&name.as_str())) {
            bail!("payout requires its own database");
        }
        MIGRATOR.run(&pool).await?;
        let scope = serde_json::to_string(&policy.scope)?;
        sqlx::query("INSERT INTO payout_binding(singleton,scope_json) VALUES(1,?) ON CONFLICT(singleton) DO NOTHING")
            .bind(&scope).execute(&pool).await?;
        let saved: String =
            sqlx::query_scalar("SELECT scope_json FROM payout_binding WHERE singleton=1")
                .fetch_one(&pool)
                .await?;
        if saved != scope {
            bail!("payout database scope changed; explicit recovery required");
        }
        Ok(Self { pool, policy })
    }

    pub async fn grant(&self, id: Uuid) -> Result<Option<Grant>> {
        sqlx::query("SELECT * FROM payout_grants WHERE id=?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .map(|row| decode_grant(&row))
            .transpose()
    }

    /// Outstanding grants and final unresolved costs remain held. Completed
    /// spend is tracked separately by the rolling admission/execution limit.
    pub async fn held_sats(&self) -> Result<u64> {
        let value: i64 =
            sqlx::query_scalar("SELECT COALESCE((SELECT SUM(g.reserved_sats) FROM payout_grants g WHERE NOT EXISTS(SELECT 1 FROM payout_outcome_acks a WHERE a.grant_id=g.id)),0)+COALESCE((SELECT SUM(held_sats) FROM payout_outcome_acks),0)-COALESCE((SELECT SUM(gross_sats) FROM payout_cost_acks),0)")
                .fetch_one(&self.pool)
                .await?;
        Ok(value.try_into()?)
    }

    pub async fn admit(
        &self,
        plan: &Plan,
        qualified: &QualifiedFunding,
        now: i64,
    ) -> Result<Grant> {
        validate_admission(plan, qualified, &self.policy, now)?;
        let plan_json = serde_json::to_string(plan)?;
        let qualification_json = serde_json::to_string(&serde_json::json!({
            "reservation":qualified.reservation(),"funding":qualified.funding(),
            "threshold_sats":qualified.threshold_sats(),"core_uid":qualified.core_uid(),
            "provider":qualified.provider(),"sources":qualified.sources(),
        }))?;
        if plan_json.len() > 4 * 1024 * 1024 || qualification_json.len() > 4 * 1024 * 1024 {
            bail!("payout grant exceeds private storage bound");
        }
        let reservation = qualified.reservation();
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE payout_clock SET observed_at=observed_at WHERE singleton=1")
            .execute(&mut *tx)
            .await?;
        if let Some(row) = sqlx::query("SELECT * FROM payout_grants WHERE id=?")
            .bind(reservation.id.to_string())
            .fetch_optional(&mut *tx)
            .await?
        {
            if row.try_get::<String, _>("plan_json")? != plan_json
                || row.try_get::<String, _>("qualification_json")? != qualification_json
            {
                bail!("payout grant retry changed immutable evidence");
            }
            let grant = decode_grant(&row)?;
            tx.commit().await?;
            return Ok(grant);
        }
        let previous: i64 =
            sqlx::query_scalar("SELECT observed_at FROM payout_clock WHERE singleton=1")
                .fetch_one(&mut *tx)
                .await?;
        if now < previous {
            bail!("payout admission clock moved backwards");
        }
        let held = outcomes::usage(&mut tx, now).await?;
        if held
            .checked_add(plan.reserved_sats)
            .is_none_or(|sum| sum > self.policy.max_sats_per_24h)
        {
            bail!("independent payout capacity exhausted");
        }
        let grant = Grant {
            id: reservation.id,
            request_id: reservation.request_id,
            plan_hash: reservation.plan_hash.clone(),
            reserved_sats: reservation.reserved_sats,
            admitted_at: now,
        };
        sqlx::query("INSERT INTO payout_grants(id,request_id,plan_hash,plan_json,qualification_json,reserved_sats,admitted_at) VALUES(?,?,?,?,?,?,?)")
            .bind(grant.id.to_string()).bind(grant.request_id.to_string()).bind(&grant.plan_hash)
            .bind(plan_json).bind(qualification_json).bind(i64::try_from(grant.reserved_sats)?)
            .bind(now).execute(&mut *tx).await?;
        for source in qualified.sources() {
            if let Some(old) = sqlx::query("SELECT * FROM payout_sources WHERE receive_id=?")
                .bind(source.receive_id.to_string())
                .fetch_optional(&mut *tx)
                .await?
            {
                let consumed = u64::try_from(old.try_get::<i64, _>("consumed_sats")?)?
                    .checked_add(source.consumed_sats)
                    .context("source consumption overflow")?;
                if old.try_get::<String, _>("request_id")? != source.receive_request_id.to_string()
                    || u64::try_from(old.try_get::<i64, _>("received_sats")?)?
                        != source.received_sats
                    || consumed > source.received_sats
                {
                    bail!("payout source identity changed or receive was overconsumed");
                }
                sqlx::query("UPDATE payout_sources SET consumed_sats=? WHERE receive_id=?")
                    .bind(i64::try_from(consumed)?)
                    .bind(source.receive_id.to_string())
                    .execute(&mut *tx)
                    .await?;
            } else {
                sqlx::query("INSERT INTO payout_sources(receive_id,request_id,received_sats,consumed_sats) VALUES(?,?,?,?)")
                    .bind(source.receive_id.to_string()).bind(source.receive_request_id.to_string())
                    .bind(i64::try_from(source.received_sats)?).bind(i64::try_from(source.consumed_sats)?)
                    .execute(&mut *tx).await?;
            }
            sqlx::query("INSERT INTO payout_source_parts(grant_id,ledger_id,receive_id,consumed_sats) VALUES(?,?,?,?)")
                .bind(grant.id.to_string()).bind(source.ledger_id).bind(source.receive_id.to_string())
                .bind(i64::try_from(source.consumed_sats)?).execute(&mut *tx).await?;
        }
        for row in plan.rows.iter().filter(|row| row.skip.is_none()) {
            let address = row
                .profile
                .as_ref()
                .and_then(|p| p.lud16.as_ref())
                .context("payee missing")?;
            sqlx::query("INSERT INTO payout_rows(grant_id,member,address,recipient_json,net_sats,fee_limit_sats,gross_sats) VALUES(?,?,?,?,?,?,?)")
                .bind(grant.id.to_string()).bind(&row.pubkey).bind(address)
                .bind(serde_json::to_string(row.recipient.as_ref().context("recipient missing")?)?)
                .bind(i64::try_from(row.net_sats)?).bind(i64::try_from(row.fee_limit_sats)?)
                .bind(i64::try_from(row.gross_sats)?).execute(&mut *tx).await?;
        }
        sqlx::query(
            "INSERT INTO payout_journal(grant_id,stage,created_at) VALUES(?,'grant_admitted',?)",
        )
        .bind(grant.id.to_string())
        .bind(now)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE payout_clock SET observed_at=? WHERE singleton=1")
            .bind(now)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(grant)
    }
}

fn decode_grant(row: &sqlx::sqlite::SqliteRow) -> Result<Grant> {
    Ok(Grant {
        id: row.try_get::<String, _>("id")?.parse()?,
        request_id: row.try_get::<String, _>("request_id")?.parse()?,
        plan_hash: row.try_get("plan_hash")?,
        reserved_sats: row.try_get::<i64, _>("reserved_sats")?.try_into()?,
        admitted_at: row.try_get("admitted_at")?,
    })
}

fn validate_admission(plan: &Plan, q: &QualifiedFunding, policy: &Policy, now: i64) -> Result<()> {
    let r = q.reservation();
    r.validate()?;
    if plan.version != 1
        || plan.owner != policy.scope.owner
        || plan.owner != r.owner
        || plan.core_uid != policy.scope.core_uid
        || q.core_uid() != policy.scope.core_uid
        || plan.epoch != policy.scope.epoch
        || plan.epoch != r.epoch
        || q.provider() != &policy.scope.provider
        || plan.request_id != r.request_id
        || plan.hash()? != r.plan_hash
        || plan.funding_hash.as_deref() != Some(&r.funding_hash)
        || q.funding().proof_hash != r.funding_hash
        || plan.real_sats != q.funding().strike_sats
        || plan.threshold_sats != q.threshold_sats()
        || plan.reserved_sats != r.reserved_sats
        || plan.root_policy_hash.as_deref() != Some(&r.policy_hash)
        || plan.member_percent == 0
        || plan.member_percent > policy.member_percent_ceiling
        || plan.cycle <= 0
        || plan.roster_revision < 0
        || plan.created_at < 0
        || now < plan.created_at
        || now < r.created_at
        || plan.rows.len() > 1000
        || q.sources().len() > usize::from(policy.max_source_parts)
    {
        bail!("plan differs from authenticated funding/root scope");
    }
    plan.planning_policy.validate()?;
    if plan.planning_policy.fee_mode != policy.fee_mode {
        bail!("frozen plan and independent payout fee modes differ");
    }
    crate::nostr_event::validate_hex(&plan.barrier_hash, 32)?;
    q.funding().validate(plan.threshold_sats)?;
    let ceiling =
        u64::try_from(u128::from(plan.real_sats) * u128::from(plan.member_percent) / 100)?
            .min(policy.max_sats_per_feed);
    if plan.budget_sats > ceiling
        || plan.reserved_sats > plan.budget_sats
        || plan.threshold_sats.checked_sub(plan.reserved_sats) != Some(plan.operations_sats)
    {
        bail!("plan exceeds independent allocation limit");
    }
    let operations = policy
        .operation_addresses
        .iter()
        .map(|a| address_identity(a))
        .collect::<Result<BTreeSet<_>>>()?;
    let mut members = BTreeSet::new();
    let mut addresses = BTreeSet::new();
    let mut callbacks = BTreeSet::new();
    let mut total = 0u64;
    for row in &plan.rows {
        crate::nostr_event::validate_public_key(&row.pubkey)?;
        if !members.insert(&row.pubkey) {
            bail!("duplicate payout member");
        }
        if row.skip.is_some() {
            if row.net_sats != 0 || row.fee_limit_sats != 0 {
                bail!("skipped row retains spending");
            }
            continue;
        }
        let profile = row.profile.as_ref().context("payout profile missing")?;
        profile.validate()?;
        crate::nostr_event::validate_hex(
            row.profile_event_id
                .as_deref()
                .context("signed profile missing")?,
            32,
        )?;
        let address = profile.lud16.as_deref().context("payout address missing")?;
        let identity = address_identity(address)?;
        let recipient = row.recipient.as_ref().context("payout recipient missing")?;
        recipient.validate(address)?;
        let amount = row
            .net_sats
            .checked_mul(1000)
            .context("payout amount overflow")?;
        if row.pubkey == plan.owner
            || operations.contains(&identity)
            || !addresses.insert(identity)
            || !callbacks.insert(recipient.callback_identity()?)
            || row.net_sats == 0
            || row.net_sats > policy.max_net_sats
            || amount > i64::MAX as u64
            || amount < recipient.min_msat
            || amount > recipient.max_msat
            || row.fee_limit_sats > policy.max_fee_sats
            || row.net_sats.checked_add(row.fee_limit_sats) != Some(row.gross_sats)
        {
            bail!("payout destination/amount violates independent policy");
        }
        total = total
            .checked_add(row.gross_sats)
            .context("payout gross overflow")?;
    }
    let mut sources = BTreeSet::new();
    let mut ledger_ids = BTreeSet::new();
    let mut real = 0u64;
    for source in q.sources() {
        if source.receive_id.is_nil()
            || source.receive_request_id.is_nil()
            || source.ledger_id <= 0
            || source.consumed_sats == 0
            || source.consumed_sats > source.received_sats
            || source.received_sats > i64::MAX as u64
            || !sources.insert(source.receive_id)
            || !ledger_ids.insert(source.ledger_id)
        {
            bail!("invalid qualified source sequence");
        }
        real = real
            .checked_add(source.consumed_sats)
            .context("payout funding overflow")?;
    }
    if total != plan.reserved_sats || real != plan.real_sats {
        bail!("payout allocation/funding total mismatch");
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod offline_fee_migration_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;
    fn policy() -> Policy {
        Policy {
            scope: Scope {
                core_uid: 10001,
                epoch: Uuid::new_v4(),
                owner: crate::nostr_event::test_event(1, 0, 100, "", vec![]).pubkey,
                provider: ProviderScope {
                    environment: "strike-production".into(),
                    account_id: Uuid::new_v4(),
                    credential_fingerprint: "aa".repeat(32),
                },
            },
            member_percent_ceiling: 0,
            max_sats_per_feed: 0,
            max_sats_per_24h: 0,
            max_fee_sats: 0,
            fee_mode: super::super::inclusive_fees::Mode::FixedAllowance,
            max_net_sats: 0,
            max_source_parts: 1,
            operation_addresses: vec![],
            receipt_relays: vec!["wss://relay.damus.io/".into()],
            execution_enabled: false,
        }
    }
    #[tokio::test]
    async fn offline_fee_migration_refuses_authority_and_never_creates_missing_journal() {
        let root = tempfile::TempDir::new().unwrap();
        let path = root.path().join("absent.db");
        let url = format!("sqlite://{}", path.display());
        for n in 0..7 {
            let mut p = policy();
            match n {
                0 => p.execution_enabled = true,
                1 => p.member_percent_ceiling = 1,
                2 => p.max_sats_per_feed = 1,
                3 => p.max_sats_per_24h = 1,
                4 => p.max_fee_sats = 1,
                5 => p.max_net_sats = 1,
                _ => {}
            }
            assert!(
                Store::migrate_fee_schema_offline(&url, &p, 10002)
                    .await
                    .is_err()
            );
            assert!(!path.exists());
        }
    }
    #[tokio::test]
    async fn offline_fee_migration_checks_scope_then_preserves_existing_bindings_under_runtime_uid()
    {
        assert_ne!(
            crate::private_ipc::uid().unwrap(),
            0,
            "ordinary fixture requires a non-admin test UID"
        );
        let root = tempfile::TempDir::new().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = root.path().join("payout.db");
        let url = format!("sqlite://{}", path.display());
        let p = policy();
        let pool = crate::sqlite::connect_durable(&url, 1).await.unwrap();
        let mut old = sqlx::migrate!("./payout-migrations");
        old.migrations = old
            .iter()
            .filter(|m| m.version <= 8)
            .cloned()
            .collect::<Vec<_>>()
            .into();
        old.run(&pool).await.unwrap();
        let scope = serde_json::to_string(&p.scope).unwrap();
        sqlx::query("INSERT INTO payout_binding(singleton,scope_json) VALUES(1,?)")
            .bind(&scope)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO payout_intake_binding VALUES(1,10002,0)")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut wrong = p.clone();
        wrong.scope.provider.account_id = Uuid::new_v4();
        assert!(
            Store::migrate_fee_schema_offline(&url, &wrong, 10002)
                .await
                .is_err()
        );
        assert!(
            Store::migrate_fee_schema_offline(&url, &p, 10003)
                .await
                .is_err()
        );
        let pool = crate::sqlite::connect_existing_durable(&url, 1)
            .await
            .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM _sqlx_migrations")
                .fetch_one(&pool)
                .await
                .unwrap(),
            7
        );
        pool.close().await;
        Store::migrate_fee_schema_offline(&url, &p, 10002)
            .await
            .unwrap();
        assert!(
            Store::migrate_fee_schema_offline(&url, &p, 10002)
                .await
                .is_err()
        );
        let pool = crate::sqlite::connect_existing_durable(&url, 1)
            .await
            .unwrap();
        crate::sqlite::require_current_schema(&pool, &MIGRATOR)
            .await
            .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT scope_json FROM payout_binding WHERE singleton=1"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            scope
        );
        assert_eq!(
            sqlx::query_as::<_, (i64, i64)>(
                "SELECT peer_uid,through FROM payout_intake_binding WHERE singleton=1"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            (10002, 0)
        );
        pool.close().await;
    }
}
