//! Preview from verified backup images only. No live journal or service authority.
use super::{Operator, Role, files, legacy::rows};
use crate::{
    core_bridge::{Batch, CoreEvent, EventBody},
    cyberherd::{
        ingress::{CoreBinding, hash},
        reward_cutover::{Decision, HistoricalFeeding},
    },
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sqlx::{Row as _, SqlitePool};
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    pub version: u8,
    pub id: Uuid,
    pub owner: String,
    pub bundle_id: Uuid,
    pub source_policy_sha256: String,
    pub decision: Decision,
    pub cutover_applied: bool,
    pub activation_authorized: bool,
}

/// Same projection used by the authenticated core bridge, over a read-only
/// accepted image. Pagination is exhaustive; no supplied UUID list is trusted.
pub(crate) async fn derive_history(
    core: &SqlitePool,
    binding: &CoreBinding,
    through: u64,
) -> Result<Vec<HistoricalFeeding>> {
    derive_history_interval(core, binding, through, true).await
}

async fn derive_history_interval(
    core: &SqlitePool,
    binding: &CoreBinding,
    through: u64,
    current_head: bool,
) -> Result<Vec<HistoricalFeeding>> {
    binding.validate()?;
    ensure!(
        through >= binding.start_after && through <= i64::MAX as u64,
        "invalid preview event boundary"
    );
    let mut tx = core.begin().await?;
    let epoch: String =
        sqlx::query_scalar("SELECT stream_id FROM overlay_identity WHERE singleton=1")
            .fetch_one(&mut *tx)
            .await?;
    ensure!(
        epoch == binding.epoch.to_string(),
        "cutover core epoch differs"
    );
    let head: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(seq),0) FROM event_log")
        .fetch_one(&mut *tx)
        .await?;
    ensure!(
        u64::try_from(head)? >= through && (!current_head || u64::try_from(head)? == through),
        "cutover must cover the accepted core head"
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM event_log WHERE seq>? AND seq<=?")
        .bind(i64::try_from(binding.start_after)?)
        .bind(i64::try_from(through)?)
        .fetch_one(&mut *tx)
        .await?;
    ensure!(
        u64::try_from(count)? == through - binding.start_after,
        "retained core event interval has a gap"
    );
    let mut after = binding.start_after;
    let mut result = Vec::new();
    loop {
        let page = sqlx::query("SELECT seq,created_at,CASE WHEN length(CAST(payload_json AS BLOB))<=32768 THEN payload_json END AS payload_json FROM event_log WHERE seq>? AND seq<=? AND event_type='feeder_confirmed' ORDER BY seq LIMIT 128")
            .bind(i64::try_from(after)?).bind(i64::try_from(through)?).fetch_all(&mut *tx).await?;
        if page.is_empty() {
            break;
        }
        for row in page {
            let seq = u64::try_from(row.try_get::<i64, _>("seq")?)?;
            let payload: Option<String> = row.try_get("payload_json")?;
            let body = crate::ledger::inspect_core_event_body(
                &mut tx,
                seq,
                "feeder_confirmed",
                payload.as_deref(),
            )
            .await?;
            let EventBody::Feeding(feeding) = &body else {
                anyhow::bail!("confirmed event lacks its original feeding source");
            };
            let item = HistoricalFeeding {
                request_id: feeding.request_id,
                core_seq: seq,
                proof_hash: hash(feeding)?,
            };
            let check = Batch {
                version: crate::core_bridge::VERSION,
                epoch: binding.epoch,
                after: seq - 1,
                through: seq,
                latest: through,
                events: vec![CoreEvent {
                    seq,
                    observed_at: row.try_get("created_at")?,
                    body,
                }],
            };
            check.validate(Some(binding.epoch), seq - 1)?;
            ensure!(
                result.len() < 16384,
                "historical inventory exceeds cutover bound"
            );
            result.push(item);
            after = seq;
        }
    }
    tx.commit().await?;
    Ok(result)
}

impl Operator {
    /// Preview only. A later native apply must bind this source to a fresh
    /// stopped-writer checkpoint and compare every retained paid/signed row.
    pub async fn preview_initial_reward_cutover(
        &self,
        bundle: &Path,
        destination: &Path,
        id: Uuid,
    ) -> Result<Preview> {
        files::absolute(destination)?;
        ensure!(
            !id.is_nil() && !destination.starts_with(bundle) && !bundle.starts_with(destination),
            "distinct preview identity/path required"
        );
        let manifest = self.verify(bundle).await?;
        ensure!(
            manifest.report.pending_feeds == 0
                && manifest.report.execution_claims == 0
                && manifest.report.unresolved_provider_payments == 0
                && manifest.report.payout_held_sats == 0,
            "physical or financial authority requires reconciliation before initial reward cutoff"
        );
        for (role, table) in [
            (Role::Core, "reward_reservations"),
            (Role::Payout, "payout_grants"),
            (Role::Signer, "signer_intents"),
        ] {
            ensure!(
                manifest.report.rows.get(&role).and_then(|r| r.get(table)) == Some(&0),
                "existing reward/signing authority blocks initial cutoff"
            );
        }
        let original = files::read_private(&bundle.join("MANIFEST.json"), 65536)?;
        let core = rows::open_named(bundle, "core.db").await?;
        let member = match rows::open_named(bundle, "membership.db").await {
            Ok(pool) => pool,
            Err(error) => {
                core.close().await;
                return Err(error);
            }
        };
        let inspected: Result<Decision> = async {
            let row = sqlx::query("SELECT binding_json,received_through FROM core_streams WHERE owner=?").bind(&self.policy.owner).fetch_one(&member).await?;
            let binding: CoreBinding = serde_json::from_str(&row.try_get::<String,_>("binding_json")?)?;
            binding.validate()?;
            ensure!(binding.epoch == self.policy.epoch && binding.core_uid == self.policy.components[&Role::Core].uid, "cutover binding differs from protected scope");
            let through = u64::try_from(sqlx::query_scalar::<_,i64>("SELECT COALESCE(MAX(seq),0) FROM event_log").fetch_one(&core).await?)?;
            ensure!(u64::try_from(row.try_get::<i64,_>("received_through")?)? <= through, "membership cursor exceeds accepted head");
            let conflicts: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM reward_plans WHERE owner=? AND (state!='no_payout' OR reservation_json IS NOT NULL))").bind(&self.policy.owner).fetch_one(&member).await?;
            ensure!(!conflicts, "existing reward plan blocks initial cutoff");
            let existing: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM operator_commands WHERE owner=? AND (command_id='initial-reward-cutover' OR command_id LIKE 'initial-reward-cutover-feed:%'))")
                .bind(&self.policy.owner).fetch_one(&member).await?;
            ensure!(!existing, "initial cutoff already retained; inspect original decision");
            let value = Decision { id, epoch:self.policy.epoch, through,
                backup_manifest_sha256:files::hash_bytes(&original), decided_at:crate::cyberherd::calendar::now()?,
                historical:derive_history(&core,&binding,through).await? };
            value.validate()?;
            ensure!(value.decided_at >= manifest.created_at, "preview clock precedes accepted backup");
            let sources=sqlx::query("SELECT source_key,proof_hash FROM core_sources WHERE owner=? AND source_key LIKE 'feeding:%'").bind(&self.policy.owner).fetch_all(&member).await?;
            for source in sources {
                let key:String=source.try_get("source_key")?;
                let request:Uuid=key.strip_prefix("feeding:").context("invalid retained feeding key")?.parse()?;
                ensure!(value.historical.iter().any(|item| item.request_id==request && item.proof_hash==source.get::<String,_>("proof_hash")), "retained membership feeding differs from native core inventory");
            }
            Ok(value)
        }.await;
        core.close().await;
        member.close().await;
        let decision = inspected?;
        ensure!(
            self.verify(bundle).await? == manifest
                && files::read_private(&bundle.join("MANIFEST.json"), 65536)? == original,
            "accepted source changed during cutover preview"
        );
        let preview = Preview {
            version: 1,
            id,
            owner: self.policy.owner.clone(),
            bundle_id: manifest.id,
            source_policy_sha256: self.policy_hash.clone(),
            decision,
            cutover_applied: false,
            activation_authorized: false,
        };
        files::new_private_directory(destination)?;
        files::write_new(
            &destination.join("INCOMPLETE"),
            b"Cutover preview is incomplete.\n",
        )?;
        files::write_new(
            &destination.join("PREVIEW.json"),
            &serde_json::to_vec_pretty(&preview)?,
        )?;
        files::sync_directory(destination)?;
        std::fs::remove_file(destination.join("INCOMPLETE"))?;
        files::sync_directory(destination)?;
        Ok(preview)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub decision_id: Uuid,
    pub epoch: Uuid,
    pub through: u64,
    pub backup_manifest_sha256: String,
    pub historical_feedings: usize,
}
impl From<&Decision> for Summary {
    fn from(value: &Decision) -> Self {
        Self {
            decision_id: value.id,
            epoch: value.epoch,
            through: value.through,
            backup_manifest_sha256: value.backup_manifest_sha256.clone(),
            historical_feedings: value.historical.len(),
        }
    }
}

/// Called after ordinary four-journal links have been checked. A signer/quote
/// cannot escape an excluded UUID check: those records require their original
/// payout grant and core reservation in the ordinary cross-journal verifier.
pub(super) async fn validate_recorded(
    core: &SqlitePool,
    member: &SqlitePool,
    policy: &super::Policy,
) -> Result<Option<Decision>> {
    use crate::cyberherd::reward_cutover::{COMMAND, PREFIX};
    let row = sqlx::query("SELECT CASE WHEN length(CAST(input_json AS BLOB))<=4194304 THEN input_json END AS input_json,result_json FROM operator_commands WHERE owner=? AND command_id=?")
        .bind(&policy.owner).bind(COMMAND).fetch_optional(member).await?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM operator_commands WHERE owner=? AND command_id LIKE 'initial-reward-cutover%'")
        .bind(&policy.owner).fetch_one(member).await?;
    let Some(row) = row else {
        ensure!(count == 0, "orphan initial reward cutoff audit");
        return Ok(None);
    };
    let raw: String = row.try_get("input_json")?;
    let value: Decision = serde_json::from_str(&raw)
        .map_err(|_| anyhow::anyhow!("invalid private cutoff decision"))?;
    value.validate()?;
    ensure!(
        value.epoch == policy.epoch
            && count == i64::try_from(value.historical.len())? + 1
            && row.try_get::<String, _>("result_json")? == serde_json::to_string(&value.receipt())?,
        "cutoff receipt/count/scope changed"
    );
    let binding: String = sqlx::query_scalar("SELECT binding_json FROM core_streams WHERE owner=?")
        .bind(&policy.owner)
        .fetch_one(member)
        .await?;
    let binding: CoreBinding = serde_json::from_str(&binding)?;
    ensure!(
        binding.epoch == policy.epoch && binding.core_uid == policy.components[&Role::Core].uid,
        "cutoff binding differs from protected scope"
    );
    ensure!(
        derive_history_interval(core, &binding, value.through, false).await? == value.historical,
        "cutoff inventory differs from retained core confirmations"
    );
    for item in &value.historical {
        let audit = sqlx::query(
            "SELECT input_json,result_json FROM operator_commands WHERE owner=? AND command_id=?",
        )
        .bind(&policy.owner)
        .bind(format!("{PREFIX}{}", item.request_id))
        .fetch_one(member)
        .await?;
        ensure!(
            audit.try_get::<String, _>("input_json")? == serde_json::to_string(item)?
                && audit.try_get::<String, _>("result_json")?
                    == serde_json::to_string(&value.receipt())?,
            "historical disposition audit differs"
        );
        let reserved: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM reward_reservations WHERE request_id=?)",
        )
        .bind(item.request_id.to_string())
        .fetch_one(core)
        .await?;
        ensure!(!reserved, "excluded feeding retains core reward authority");
        if let Some(plan) = sqlx::query("SELECT state,reservation_json,plan_hash,plan_json FROM reward_plans WHERE owner=? AND request_id=?")
            .bind(&policy.owner).bind(item.request_id.to_string()).fetch_optional(member).await? {
            let raw:String=plan.try_get("plan_json")?;
            ensure!(raw.len()<=4194304,"excluded plan exceeds bound");
            let frozen:crate::cyberherd::rewards::Plan=serde_json::from_str(&raw)?;
            ensure!(plan.try_get::<String,_>("state")?=="no_payout"
                && plan.try_get::<Option<String>,_>("reservation_json")?.is_none()
                && frozen.owner==policy.owner && frozen.epoch==policy.epoch && frozen.request_id==item.request_id
                && frozen.hash()?==plan.try_get::<String,_>("plan_hash")? && frozen.reserved_sats==0,
                "excluded feeding retains nonterminal or conflicting plan");
        }
        if let Some(raw) = sqlx::query_scalar::<_, String>(
            "SELECT payload_json FROM ingress WHERE owner=? AND source_key=? AND kind='feeding'",
        )
        .bind(&policy.owner)
        .bind(format!("feeding:{}", item.request_id))
        .fetch_optional(member)
        .await?
        {
            ensure!(raw.len() <= 4194304, "excluded input exceeds bound");
            let feeding: crate::core_bridge::Feeding = serde_json::from_str(&raw)?;
            ensure!(
                feeding.request_id == item.request_id && hash(&feeding)? == item.proof_hash,
                "excluded intake contradicts original core proof"
            );
        }
    }
    Ok(Some(value))
}
