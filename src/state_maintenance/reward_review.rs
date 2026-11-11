//! Private review of verified backup images. No planning, reservation or resume.
use super::{Manifest, Operator, Policy, Role, files, legacy::rows};
use crate::{
    cyberherd::{
        ingress::{CoreBinding, FeedingBarrier, hash},
        membership::Settings,
        rewards::Plan,
    },
    reward_funding::Reservation,
};
use anyhow::{Context, Result, ensure};
use futures_util::TryStreamExt as _;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sqlx::{Row as _, SqlitePool, sqlite::SqliteRow};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use uuid::Uuid;

const EXAMPLES: usize = 32;
const JSON_BOUND: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    UnplannedZeroAllocation,
    UnplannedNoStrikeFunding,
    UnplannedFundedAllocation,
    NoPayout,
    AwaitingReservation,
    Reserved,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Example {
    pub request_id: Uuid,
    pub ingress_seq: i64,
    pub cycle: i64,
    pub revision: i64,
    pub frozen_member_percent: u8,
    pub strike_sats: u64,
    pub stage: Stage,
    pub plan_hash: Option<String>,
    pub reserved_sats: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RewardReview {
    pub version: u8,
    pub id: Uuid,
    pub bundle_id: Uuid,
    pub bundle_created_at: i64,
    pub bundle_manifest_sha256: String,
    pub reviewed_at: i64,
    pub owner: String,
    pub epoch: Uuid,
    pub stored_member_percent: u8,
    pub received_through: u64,
    pub core_event_head: u64,
    pub pending_ingress: u64,
    pub pending_feeding_ingress: u64,
    pub pending_feeds: u64,
    pub stages: BTreeMap<Stage, u64>,
    pub unfinalized_core_reservations: u64,
    pub unresolved_provider_payments: u64,
    pub payout_held_sats: u64,
    pub examples: Vec<Example>,
    pub additional_examples: u64,
    pub all_barriers_examined: bool,
    pub activation_authorized: bool,
    pub external_reconciliation_required: bool,
}

impl Operator {
    /// Inspect only an accepted bundle, including its committed copied WAL.
    /// Existing manifests/backup images never acquire fields or changed bytes.
    pub async fn review_rewards(&self, bundle: &Path, destination: &Path) -> Result<RewardReview> {
        files::absolute(destination)?;
        ensure!(
            !destination.starts_with(bundle) && !bundle.starts_with(destination),
            "reward report must be separate from its accepted backup"
        );
        let manifest = self.verify(bundle).await?;
        let original = files::read_private(&bundle.join("MANIFEST.json"), 65536)?;
        let review = inspect(
            bundle,
            &self.policy,
            &manifest,
            files::hash_bytes(&original),
        )
        .await?;
        ensure!(
            self.verify(bundle).await? == manifest,
            "backup changed during reward review"
        );
        ensure!(
            files::read_private(&bundle.join("MANIFEST.json"), 65536)? == original,
            "backup manifest changed during reward review"
        );
        files::new_private_directory(destination)?;
        files::write_new(
            &destination.join("INCOMPLETE"),
            b"Reward review is incomplete.\n",
        )?;
        files::write_new(
            &destination.join("REVIEW.json"),
            &serde_json::to_vec_pretty(&review)?,
        )?;
        files::check_images(bundle, &manifest.images, self.policy.max_component_bytes)?;
        ensure!(
            files::read_private(&bundle.join("MANIFEST.json"), 65536)? == original,
            "backup manifest changed while writing reward report"
        );
        files::sync_directory(destination)?;
        std::fs::remove_file(destination.join("INCOMPLETE"))?;
        files::sync_directory(destination)?;
        Ok(review)
    }
}

async fn inspect(
    directory: &Path,
    policy: &Policy,
    manifest: &Manifest,
    digest: String,
) -> Result<RewardReview> {
    let member = rows::open_named(directory, "membership.db").await?;
    let core = match rows::open_named(directory, "core.db").await {
        Ok(pool) => pool,
        Err(error) => {
            member.close().await;
            return Err(error);
        }
    };
    let result = inspect_pools(&member, &core, policy, manifest, digest).await;
    member.close().await;
    core.close().await;
    result
}

async fn count(pool: &SqlitePool, query: &str, owner: &str) -> Result<u64> {
    Ok(u64::try_from(
        sqlx::query_scalar::<_, i64>(query)
            .bind(owner)
            .fetch_one(pool)
            .await?,
    )?)
}

fn bounded(row: &SqliteRow, name: &str) -> Result<String> {
    let value: String = row.try_get(name)?;
    ensure!(
        value.len() <= JSON_BOUND,
        "reward record exceeds review bound"
    );
    Ok(value)
}

fn decode<T: DeserializeOwned>(raw: &str) -> Result<T> {
    serde_json::from_str(raw).map_err(|_| anyhow::anyhow!("invalid typed private reward record"))
}

async fn inspect_pools(
    member: &SqlitePool,
    core: &SqlitePool,
    policy: &Policy,
    manifest: &Manifest,
    digest: String,
) -> Result<RewardReview> {
    let settings: Settings = decode(&sqlx::query_scalar::<_, String>(
        "SELECT CASE WHEN length(CAST(settings_json AS BLOB))<=65536 THEN settings_json END FROM owners WHERE owner=?"
    ).bind(&policy.owner).fetch_one(member).await?)?;
    settings.validate()?;
    let row = sqlx::query("SELECT CASE WHEN length(CAST(binding_json AS BLOB))<=65536 THEN binding_json END AS binding_json,received_through FROM core_streams WHERE owner=?")
        .bind(&policy.owner).fetch_one(member).await?;
    let binding: CoreBinding = decode(&bounded(&row, "binding_json")?)?;
    binding.validate()?;
    ensure!(
        binding.epoch == policy.epoch && binding.core_uid == policy.components[&Role::Core].uid,
        "reward binding differs from verified backup scope"
    );
    let mut report = RewardReview {
        version: 1, id: Uuid::new_v4(), bundle_id: manifest.id, bundle_created_at: manifest.created_at,
        bundle_manifest_sha256: digest, reviewed_at: crate::cyberherd::calendar::now()?,
        owner: policy.owner.clone(), epoch: policy.epoch, stored_member_percent: settings.member_percent,
        received_through: u64::try_from(row.try_get::<i64, _>("received_through")?)?,
        core_event_head: u64::try_from(sqlx::query_scalar::<_, i64>("SELECT COALESCE(MAX(seq),0) FROM event_log").fetch_one(core).await?)?,
        pending_ingress: count(member, "SELECT COUNT(*) FROM ingress WHERE owner=? AND state='pending'", &policy.owner).await?,
        pending_feeding_ingress: count(member, "SELECT COUNT(*) FROM ingress WHERE owner=? AND state='pending' AND kind='feeding'", &policy.owner).await?,
        pending_feeds: manifest.report.pending_feeds,
        unfinalized_core_reservations: count(core, "SELECT COUNT(*) FROM reward_reservations r WHERE r.owner=? AND NOT EXISTS(SELECT 1 FROM reward_outcome_finals f WHERE f.reservation_id=r.id)", &policy.owner).await?,
        unresolved_provider_payments: manifest.report.unresolved_provider_payments,
        payout_held_sats: manifest.report.payout_held_sats,
        stages: BTreeMap::new(), examples: Vec::new(), additional_examples: 0,
        all_barriers_examined: false, activation_authorized: false, external_reconciliation_required: true,
    };
    ensure!(
        report.received_through <= report.core_event_head,
        "membership cursor exceeds backup core head"
    );
    // Read every row, but retain at most 32 private examples. An oversized record
    // refuses the report; it is never silently skipped or declared zero-valued.
    let mut stream = sqlx::query(r#"SELECT b.request_id,b.ingress_seq,
CASE WHEN length(CAST(b.feeding_json AS BLOB))<=4194304 THEN b.feeding_json END AS feeding_json,
CASE WHEN length(CAST(b.snapshot_json AS BLOB))<=4194304 THEN b.snapshot_json END AS snapshot_json,
CASE WHEN length(CAST(b.conversations_json AS BLOB))<=4194304 THEN b.conversations_json END AS conversations_json,
i.kind,i.state AS ingress_state,CASE WHEN length(CAST(i.payload_json AS BLOB))<=4194304 THEN i.payload_json END AS payload_json,
p.plan_hash,p.state,CASE WHEN length(CAST(p.plan_json AS BLOB))<=4194304 THEN p.plan_json END AS plan_json,
CASE WHEN length(CAST(p.reservation_json AS BLOB))<=4194304 THEN p.reservation_json END AS reservation_json
FROM feeding_barriers b LEFT JOIN ingress i ON i.seq=b.ingress_seq AND i.owner=b.owner
LEFT JOIN reward_plans p ON p.owner=b.owner AND p.request_id=b.request_id
WHERE b.owner=? ORDER BY b.ingress_seq"#).bind(&policy.owner).fetch(member);
    let mut previous = 0;
    while let Some(row) = stream.try_next().await? {
        let id: String = row.try_get("request_id")?;
        let feeding_json = bounded(&row, "feeding_json")?;
        let barrier = FeedingBarrier {
            ingress_seq: row.try_get("ingress_seq")?,
            feeding: decode(&feeding_json)?,
            snapshot: decode(&bounded(&row, "snapshot_json")?)?,
            conversations: decode(&bounded(&row, "conversations_json")?)?,
        };
        ensure!(
            barrier.ingress_seq > previous
                && row.try_get::<String, _>("kind")? == "feeding"
                && row.try_get::<String, _>("ingress_state")? == "done"
                && row.try_get::<String, _>("payload_json")? == feeding_json,
            "feeding barrier lost its completed ordered input"
        );
        previous = barrier.ingress_seq;
        let example = classify(core, policy, &id, &barrier, &row).await?;
        *report.stages.entry(example.stage).or_default() += 1;
        if example.stage != Stage::NoPayout {
            if report.examples.len() < EXAMPLES {
                report.examples.push(example);
            } else {
                report.additional_examples += 1;
            }
        }
    }
    drop(stream);
    ensure!(
        report.stages.values().sum::<u64>()
            == count(
                member,
                "SELECT COUNT(*) FROM feeding_barriers WHERE owner=?",
                &policy.owner
            )
            .await?,
        "not every retained feeding barrier was examined"
    );
    report.all_barriers_examined = true;
    Ok(report)
}

async fn classify(
    core: &SqlitePool,
    policy: &Policy,
    id: &str,
    barrier: &FeedingBarrier,
    row: &SqliteRow,
) -> Result<Example> {
    let feeding = &barrier.feeding;
    let snapshot = &barrier.snapshot;
    snapshot.settings.validate()?;
    ensure!(
        !feeding.request_id.is_nil()
            && feeding.request_id.to_string() == id
            && feeding.threshold_sats > 0
            && feeding.threshold_sats <= i64::MAX as u64
            && feeding.confirmed_at >= 0
            && snapshot.owner == policy.owner
            && snapshot.cycle > 0
            && snapshot.cycle_start >= 0
            && snapshot.revision >= 0,
        "invalid frozen feeding scope"
    );
    for (key, context) in &barrier.conversations {
        crate::nostr_event::validate_public_key(key)?;
        context.validate(&policy.owner)?;
    }
    let confirmation =
        sqlx::query("SELECT status,threshold_sats,resolved_at FROM feed_attempts WHERE id=?")
            .bind(id)
            .fetch_one(core)
            .await?;
    ensure!(
        confirmation.try_get::<String, _>("status")? == "confirmed"
            && u64::try_from(confirmation.try_get::<i64, _>("threshold_sats")?)?
                == feeding.threshold_sats
            && confirmation.try_get::<Option<i64>, _>("resolved_at")? == Some(feeding.confirmed_at),
        "frozen feeding differs from original core confirmation"
    );
    if let Some(funding) = &feeding.funding {
        funding.validate(feeding.threshold_sats)?;
        let mut tx = core.begin().await?;
        ensure!(
            crate::ledger::inspect_confirmed_reward_funding(&mut tx, feeding.request_id)
                .await?
                .as_ref()
                == Some(funding),
            "feeding funding differs from original consumed core lots"
        );
        tx.commit().await?;
    }
    let original = sqlx::query("SELECT * FROM reward_reservations WHERE request_id=?")
        .bind(id)
        .fetch_optional(core)
        .await?;
    let original = original
        .as_ref()
        .map(crate::ledger::inspect_reward_reservation)
        .transpose()?;
    let strike_sats = feeding.funding.as_ref().map_or(0, |f| f.strike_sats);
    let plan_hash: Option<String> = row.try_get("plan_hash")?;
    let (stage, reserved_sats) = if let Some(plan_hash) = &plan_hash {
        let plan: Plan = decode(&bounded(row, "plan_json")?)?;
        ensure!(
            plan.hash()? == *plan_hash
                && plan.owner == policy.owner
                && plan.epoch == policy.epoch
                && plan.core_uid == policy.components[&Role::Core].uid
                && plan.request_id == feeding.request_id
                && plan.barrier_hash == hash(barrier)?
                && plan.cycle == snapshot.cycle
                && plan.roster_revision == snapshot.revision
                && plan.threshold_sats == feeding.threshold_sats
                && plan.real_sats == strike_sats
                && plan.funding_hash == feeding.funding.as_ref().map(|f| f.proof_hash.clone())
                && plan.member_percent <= snapshot.settings.member_percent
                && plan.created_at >= feeding.confirmed_at
                && u128::from(plan.budget_sats)
                    <= u128::from(strike_sats) * u128::from(plan.member_percent) / 100
                && plan.reserved_sats <= plan.budget_sats
                && plan.operations_sats.checked_add(plan.reserved_sats)
                    == Some(plan.threshold_sats),
            "retained plan differs from its frozen barrier or budget"
        );
        plan.planning_policy.validate()?;
        let mut members = BTreeSet::new();
        let mut reserved = 0u64;
        for row in &plan.rows {
            crate::nostr_event::validate_public_key(&row.pubkey)?;
            ensure!(
                members.insert(&row.pubkey),
                "retained plan repeats a member"
            );
            let member = snapshot
                .roster
                .members
                .get(&row.pubkey)
                .context("retained plan member missing from frozen roster")?;
            ensure!(
                member.active
                    && !snapshot.roster.banned.contains(&row.pubkey)
                    && member.amount_sats == row.amount_sats
                    && snapshot
                        .roster
                        .awards
                        .get(&row.pubkey)
                        .copied()
                        .unwrap_or_default()
                        == row.awards,
                "retained plan member differs from frozen membership"
            );
            if row.skip.is_some() {
                ensure!(
                    row.net_sats == 0 && row.fee_limit_sats == 0,
                    "skipped retained row has payment authority"
                );
            } else {
                ensure!(
                    row.net_sats > 0
                        && row.net_sats <= i64::MAX as u64 / 1000
                        && row.net_sats.checked_add(row.fee_limit_sats) == Some(row.gross_sats),
                    "retained row has inconsistent payment amounts"
                );
                reserved = reserved
                    .checked_add(row.gross_sats)
                    .context("retained row sum overflow")?;
            }
        }
        ensure!(
            reserved == plan.reserved_sats,
            "retained plan and row reservation sums differ"
        );
        let reservation: Option<Reservation> = row
            .try_get::<Option<String>, _>("reservation_json")?
            .map(|s| decode(&s))
            .transpose()?;
        if let Some(r) = &original {
            ensure!(
                r.owner == policy.owner
                    && r.epoch == policy.epoch
                    && r.plan_hash == *plan_hash
                    && r.reserved_sats == plan.reserved_sats
                    && Some(&r.funding_hash) == plan.funding_hash.as_ref(),
                "core reservation differs from retained plan"
            );
        }
        let stage = match row.try_get::<&str, _>("state")? {
            "no_payout"
                if plan.reserved_sats == 0 && reservation.is_none() && original.is_none() =>
            {
                Stage::NoPayout
            }
            "awaiting_reservation" if plan.reserved_sats > 0 && reservation.is_none() => {
                Stage::AwaitingReservation
            }
            "reserved"
                if plan.reserved_sats > 0 && reservation.is_some() && reservation == original =>
            {
                Stage::Reserved
            }
            _ => anyhow::bail!("retained plan/reservation state is inconsistent"),
        };
        (stage, plan.reserved_sats)
    } else {
        ensure!(
            original.is_none(),
            "unplanned feeding already has a core reservation"
        );
        (
            if snapshot.settings.member_percent == 0 {
                Stage::UnplannedZeroAllocation
            } else if strike_sats == 0 {
                Stage::UnplannedNoStrikeFunding
            } else {
                Stage::UnplannedFundedAllocation
            },
            0,
        )
    };
    Ok(Example {
        request_id: feeding.request_id,
        ingress_seq: barrier.ingress_seq,
        cycle: snapshot.cycle,
        revision: snapshot.revision,
        frozen_member_percent: snapshot.settings.member_percent,
        strike_sats,
        stage,
        plan_hash,
        reserved_sats,
    })
}

#[cfg(test)]
mod tests;
