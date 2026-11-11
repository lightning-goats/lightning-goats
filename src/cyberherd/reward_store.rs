//! Private immutable plans, sourced from completed ordered feeding barriers.
use anyhow::{Context, Result, bail};
use sqlx::{Row as _, Sqlite, Transaction};
use uuid::Uuid;

use super::Store;
use crate::{
    core_bridge::AuthenticatedReservation,
    cyberherd::{
        ingress::{CoreBinding, FeedingBarrier, validate_owner},
        rewards::{self, Plan, Preparation, ProfileRead, State, StoredPlan},
        targets::AuthenticatedProfile,
    },
    nostr_event::VerifiedEvent,
    payout::recipient::Recipient,
};

impl Store {
    #[cfg(unix)]
    pub(crate) async fn next_reward_export(
        &self,
        owner: &str,
        after: i64,
    ) -> Result<Option<super::super::payout_bridge::Export>> {
        validate_owner(owner)?;
        if after < 0 {
            bail!("invalid plan cursor");
        }
        let mut tx = self.pool.begin().await?;
        // Select the earliest barrier even if planning/reservation is still
        // pending. Exporting later ready plans must not skip an earlier one.
        let row=sqlx::query("SELECT b.ingress_seq,p.* FROM feeding_barriers b LEFT JOIN reward_plans p ON p.owner=b.owner AND p.request_id=b.request_id WHERE b.owner=? AND b.ingress_seq>? ORDER BY b.ingress_seq LIMIT 1")
            .bind(owner).bind(after).fetch_optional(&mut *tx).await?;
        let Some(row) = row else { return Ok(None) };
        let state: Option<String> = row.try_get("state")?;
        if state
            .as_deref()
            .is_none_or(|state| state == "awaiting_reservation")
        {
            return Ok(None);
        }
        let plan = decode(&row)?;
        let export = super::super::payout_bridge::Export {
            sequence: row.try_get("ingress_seq")?,
            hash: plan.hash,
            plan: plan.plan,
            reservation: plan.reservation,
        };
        export.validate()?;
        tx.commit().await?;
        Ok(Some(export))
    }
    pub(in crate::cyberherd) async fn stage_reward_recipient(
        &self,
        owner: &str,
        id: Uuid,
        profile: &ProfileRead,
        recipient: Option<&Recipient>,
    ) -> Result<()> {
        let address = profile
            .address()
            .context("recipient requires a signed payout address")?;
        if let Some(r) = recipient {
            r.validate(address)?;
        }
        sqlx::query("INSERT INTO reward_recipient_reads(owner,request_id,pubkey,head_id,address,recipient_json) VALUES(?,?,?,?,?,?) ON CONFLICT(owner,request_id,pubkey) DO UPDATE SET head_id=excluded.head_id,address=excluded.address,recipient_json=excluded.recipient_json")
            .bind(owner).bind(id.to_string()).bind(&profile.pubkey).bind(profile.head_id.as_ref().context("missing signed profile")?).bind(address)
            .bind(recipient.map(serde_json::to_string).transpose()?).execute(&self.pool).await?;
        Ok(())
    }

    /// Outer None = not staged for this signed head; inner None = GET failed.
    pub(in crate::cyberherd) async fn staged_reward_recipient(
        &self,
        owner: &str,
        id: Uuid,
        profile: &ProfileRead,
    ) -> Result<Option<Option<Recipient>>> {
        let row = sqlx::query("SELECT recipient_json FROM reward_recipient_reads WHERE owner=? AND request_id=? AND pubkey=? AND head_id=? AND address=?")
            .bind(owner).bind(id.to_string()).bind(&profile.pubkey).bind(&profile.head_id).bind(profile.address()).fetch_optional(&self.pool).await?;
        row.map(|row| -> Result<_> {
            Ok(row
                .try_get::<Option<String>, _>("recipient_json")?
                .map(|s| serde_json::from_str(&s))
                .transpose()?)
        })
        .transpose()
    }

    /// Completion is supplied by authenticated discovery, independently of
    /// signed-head replacement. Failed reads invalidate a prior successful read.
    pub(in crate::cyberherd) async fn record_reward_profile_read(
        &self,
        owner: &str,
        key: &str,
        available: bool,
        now: i64,
    ) -> Result<()> {
        validate_owner(owner)?;
        crate::nostr_event::validate_public_key(key)?;
        if now < 0 {
            bail!("invalid profile observation time");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let head: Option<String> =
            sqlx::query_scalar("SELECT event_id FROM profile_heads WHERE owner=? AND pubkey=?")
                .bind(owner)
                .bind(key)
                .fetch_optional(&mut *tx)
                .await?;
        sqlx::query("INSERT INTO reward_profile_reads(owner,pubkey,head_id,observed_at,available) VALUES(?,?,?,?,?) ON CONFLICT(owner,pubkey) DO UPDATE SET head_id=excluded.head_id,observed_at=excluded.observed_at,available=excluded.available WHERE excluded.observed_at>=reward_profile_reads.observed_at")
            .bind(owner).bind(key).bind(head).bind(now).bind(available).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn reward_profile_read(
        &self,
        owner: &str,
        key: &str,
        now: i64,
        max_age: u32,
    ) -> Result<Option<ProfileRead>> {
        let mut tx = self.pool.begin().await?;
        let result = profile_read(&mut tx, owner, key, now, max_age).await?;
        tx.commit().await?;
        Ok(result)
    }

    /// Oldest unfinished confirmed feeding; later membership changes can never
    /// replace this barrier's frozen roster. Reserved/no-payout rows are closed.
    pub async fn next_reward_feeding(&self, owner: &str) -> Result<Option<Uuid>> {
        validate_owner(owner)?;
        let id: Option<String> = sqlx::query_scalar("SELECT b.request_id FROM feeding_barriers b LEFT JOIN reward_plans p ON b.owner=p.owner AND b.request_id=p.request_id WHERE b.owner=? AND (p.state IS NULL OR p.state='awaiting_reservation') ORDER BY b.ingress_seq LIMIT 1")
            .bind(owner).fetch_optional(&self.pool).await?;
        id.map(|id| id.parse().map_err(Into::into)).transpose()
    }

    pub async fn reward_plan(&self, owner: &str, id: Uuid) -> Result<Option<StoredPlan>> {
        validate_owner(owner)?;
        let row = sqlx::query("SELECT * FROM reward_plans WHERE owner=? AND request_id=?")
            .bind(owner)
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(decode).transpose()
    }

    /// Fetching recipient terms happens outside this transaction. Compare the
    /// exact signed profile/read again before freezing; changed heads retry.
    pub async fn freeze_reward_plan(
        &self,
        owner: &str,
        id: Uuid,
        preparation: Preparation<'_>,
    ) -> Result<StoredPlan> {
        let Preparation {
            policy,
            allowance,
            profiles,
            recipients,
            now,
        } = preparation;
        validate_owner(owner)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(row) = sqlx::query("SELECT * FROM reward_plans WHERE owner=? AND request_id=?")
            .bind(owner)
            .bind(id.to_string())
            .fetch_optional(&mut *tx)
            .await?
        {
            let old = decode(&row)?;
            tx.commit().await?;
            return Ok(old);
        }
        let barrier = sqlx::query("SELECT * FROM feeding_barriers WHERE owner=? AND request_id=?")
            .bind(owner)
            .bind(id.to_string())
            .fetch_one(&mut *tx)
            .await?;
        let barrier = FeedingBarrier {
            ingress_seq: barrier.try_get("ingress_seq")?,
            feeding: serde_json::from_str(&barrier.try_get::<String, _>("feeding_json")?)?,
            snapshot: serde_json::from_str(&barrier.try_get::<String, _>("snapshot_json")?)?,
            conversations: serde_json::from_str(
                &barrier.try_get::<String, _>("conversations_json")?,
            )?,
        };
        if barrier.snapshot.owner != owner || barrier.feeding.request_id != id {
            bail!("stored feeding scope mismatch");
        }
        let binding: String =
            sqlx::query_scalar("SELECT binding_json FROM core_streams WHERE owner=?")
                .bind(owner)
                .fetch_one(&mut *tx)
                .await?;
        let binding: CoreBinding = serde_json::from_str(&binding)?;
        binding.validate()?;
        if allowance.is_some_and(|a| a.core_uid() != binding.core_uid) {
            bail!("reward allowance from wrong core identity");
        }
        for read in profiles {
            let current = profile_read(
                &mut tx,
                owner,
                &read.pubkey,
                now,
                policy.profile_max_age_seconds,
            )
            .await?
            .context("reward profile unavailable or changed while resolving recipient")?;
            if current.head_id != read.head_id
                || current.observed_at != read.observed_at
                || current.profile != read.profile
            {
                bail!("reward profile changed during recipient resolution");
            }
        }
        let mut plan = rewards::build(
            &barrier,
            &binding,
            policy,
            allowance.map(|a| a.allowance()),
            profiles,
            recipients,
            now,
        )?;
        let catalog = super::load_catalog(&mut tx, owner).await?;
        for row in &mut plan.rows {
            if let Some(conversation) = &row.conversation {
                conversation.validate(owner)?;
            }
            if row.skip.is_none() {
                row.zap_wording = catalog
                    .select("cyber_herd_zap_request", &format!("{}:{}", id, row.pubkey))?
                    .map(|template| super::rewards::ZapWording {
                        template: template.to_owned(),
                        name: row
                            .profile
                            .as_ref()
                            .map_or("Anon", |p| p.name.as_str())
                            .to_owned(),
                    });
                row.zap_content = row
                    .zap_wording
                    .as_ref()
                    .map(|wording| wording.render(row.net_sats))
                    .transpose()?;
                if row
                    .zap_content
                    .as_ref()
                    .is_some_and(|text| text.len() > 2048)
                {
                    bail!("outgoing zap wording exceeds the invoice intent bound");
                }
            }
        }
        let hash = plan.hash()?;
        let state = if plan.reserved_sats == 0 {
            "no_payout"
        } else {
            "awaiting_reservation"
        };
        sqlx::query("INSERT INTO reward_plans(owner,request_id,plan_hash,plan_json,state) VALUES(?,?,?,?,?)")
            .bind(owner).bind(id.to_string()).bind(&hash).bind(serde_json::to_string(&plan)?).bind(state).execute(&mut *tx).await?;
        for row in &plan.rows {
            if matches!(
                row.skip,
                Some(
                    rewards::Skip::ZeroShare
                        | rewards::Skip::BelowMinimum
                        | rewards::Skip::RecipientMinimum
                        | rewards::Skip::RecipientMaximum
                        | rewards::Skip::PolicyMaximum
                        | rewards::Skip::RecipientUnavailable
                )
            ) {
                let payload = serde_json::json!({"request_id":id,"breakdown":row});
                sqlx::query("INSERT INTO domain_events(owner,cycle,source_id,event_type,snapshot_json) VALUES(?,?,?,'payout_skipped',?)")
                    .bind(owner).bind(plan.cycle).bind(format!("payout-skipped:{id}:{}",row.pubkey)).bind(serde_json::to_string(&payload)?).execute(&mut *tx).await?;
            }
        }
        tx.commit().await?;
        Ok(StoredPlan {
            hash,
            plan,
            state: if state == "no_payout" {
                State::NoPayout
            } else {
                State::AwaitingReservation
            },
            reservation: None,
        })
    }

    pub async fn attach_reward_reservation(
        &self,
        owner: &str,
        approved: &AuthenticatedReservation,
    ) -> Result<StoredPlan> {
        let r = approved.reservation();
        r.validate()?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query("SELECT * FROM reward_plans WHERE owner=? AND request_id=?")
            .bind(owner)
            .bind(r.request_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
        let mut plan = decode(&row)?;
        if r.owner != owner
            || r.epoch != plan.plan.epoch
            || approved.core_uid() != plan.plan.core_uid
            || r.plan_hash != plan.hash
            || Some(&r.funding_hash) != plan.plan.funding_hash.as_ref()
            || r.reserved_sats != plan.plan.reserved_sats
            || plan.state == State::NoPayout
        {
            bail!("authenticated reservation contradicts frozen reward plan");
        }
        if let Some(old) = &plan.reservation {
            if old != r {
                bail!("different reservation already attached");
            }
        } else {
            sqlx::query("UPDATE reward_plans SET state='reserved',reservation_json=? WHERE owner=? AND request_id=? AND state='awaiting_reservation'")
                .bind(serde_json::to_string(r)?).bind(owner).bind(r.request_id.to_string()).execute(&mut *tx).await?;
            plan.reservation = Some(r.clone());
            plan.state = State::Reserved;
        }
        tx.commit().await?;
        Ok(plan)
    }
}

async fn profile_read(
    tx: &mut Transaction<'_, Sqlite>,
    owner: &str,
    key: &str,
    now: i64,
    max_age: u32,
) -> Result<Option<ProfileRead>> {
    validate_owner(owner)?;
    crate::nostr_event::validate_public_key(key)?;
    let row = sqlx::query("SELECT r.head_id,r.observed_at,r.available,h.event_id,e.event_json FROM reward_profile_reads r LEFT JOIN profile_heads h ON h.owner=r.owner AND h.pubkey=r.pubkey LEFT JOIN observed_events e ON e.owner=h.owner AND e.event_id=h.event_id WHERE r.owner=? AND r.pubkey=?")
        .bind(owner).bind(key).fetch_optional(&mut **tx).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let at: i64 = row.try_get("observed_at")?;
    let head_id: Option<String> = row.try_get("head_id")?;
    let current: Option<String> = row.try_get("event_id")?;
    if !row.try_get::<bool, _>("available")?
        || at > now
        || now - at > i64::from(max_age)
        || head_id != current
    {
        return Ok(None);
    }
    let json: Option<String> = row.try_get("event_json")?;
    let profile = json
        .map(|json| -> Result<_> {
            Ok(
                AuthenticatedProfile::parse(VerifiedEvent::from_json(&json)?, key, now)?
                    .profile()
                    .clone(),
            )
        })
        .transpose()?;
    Ok(Some(ProfileRead {
        pubkey: key.into(),
        head_id,
        profile,
        observed_at: at,
    }))
}

pub(super) fn decode(row: &sqlx::sqlite::SqliteRow) -> Result<StoredPlan> {
    let plan: Plan = serde_json::from_str(&row.try_get::<String, _>("plan_json")?)?;
    let hash: String = row.try_get("plan_hash")?;
    if plan.hash()? != hash
        || plan.owner != row.try_get::<String, _>("owner")?
        || plan.request_id.to_string() != row.try_get::<String, _>("request_id")?
    {
        bail!("frozen reward plan identity/hash mismatch");
    }
    let state = match row.try_get::<&str, _>("state")? {
        "awaiting_reservation" => State::AwaitingReservation,
        "reserved" => State::Reserved,
        "no_payout" => State::NoPayout,
        _ => bail!("invalid reward plan state"),
    };
    let reservation = row
        .try_get::<Option<String>, _>("reservation_json")?
        .map(|r| serde_json::from_str(&r))
        .transpose()?;
    Ok(StoredPlan {
        plan,
        hash,
        state,
        reservation,
    })
}
