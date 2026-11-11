//! Durable quote/execute correlation. No provider call can erase uncertainty.
use super::invoices::{load_invoice, touch_clock, write_lock};
use super::*;
use crate::{core_bridge::AuthenticatedFundingPage, payout::provider::*};
use sqlx::{Sqlite, Transaction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Unquoted,
    QuoteUnknown,
    QuoteReady,
    ExecutionUnknown,
    Pending,
    Completed,
    Failed,
    Conflicted,
}

impl Store {
    pub async fn saved_fee_observation(
        &self,
        grant: Uuid,
        member: &str,
    ) -> Result<Option<(u8, FeeObservation)>> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query("SELECT round,provider_quote_id,gross_sats,invoice_net_sats,fee_sats,total_sats,observed_at,document_json FROM payout_fee_observations WHERE grant_id=? AND member=? ORDER BY round DESC LIMIT 1")
            .bind(grant.to_string()).bind(member).fetch_optional(&mut *tx).await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        let observation =
            restore_fee_observation(row.try_get::<String, _>("document_json")?.as_bytes())?;
        let (doc, bound_grant, bound_member, probe_round) =
            load_fee_doc(&mut tx, observation.0.request.id, &self.policy.scope.owner).await?;
        let saved_round = row.try_get::<i64, _>("round")?;
        if doc != observation.0.request
            || doc.scope != self.policy.scope.provider
            || bound_grant != grant
            || bound_member != member
            || probe_round.is_some_and(|r| i64::from(r) != saved_round)
            || row.try_get::<String, _>("provider_quote_id")? != observation.0.quote_id.to_string()
            || u64::try_from(row.try_get::<i64, _>("gross_sats")?)? != doc.gross_sats
            || u64::try_from(row.try_get::<i64, _>("invoice_net_sats")?)? != doc.amount_sats
            || u64::try_from(row.try_get::<i64, _>("fee_sats")?)? != observation.0.fee_sats
            || u64::try_from(row.try_get::<i64, _>("total_sats")?)? != observation.0.total_sats
            || row.try_get::<i64, _>("observed_at")? != observation.0.observed_at
        {
            bail!("saved fee observation differs from its original row and request");
        }
        let round = u8::try_from(row.try_get::<i64, _>("round")?)?;
        if round > 7 {
            bail!("fee observation round exceeds bound");
        }
        tx.commit().await?;
        Ok(Some((round, observation)))
    }

    /// Preserve fee discovery and retire the original, never-executed quote in
    /// one transaction. The returned arithmetic decision is not execution
    /// authority; reduced invoices still require their own exact signed bytes.
    pub async fn record_fee_observation(
        &self,
        observation: &FeeObservation,
        round: u8,
        now: i64,
    ) -> Result<crate::payout::inclusive_fees::Decision> {
        observation.0.validate()?;
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        let (doc, grant, member, probe_round) =
            load_fee_doc(&mut tx, observation.0.request.id, &self.policy.scope.owner).await?;
        if doc != observation.0.request
            || doc.scope != self.policy.scope.provider
            || probe_round.is_some_and(|r| r != round)
        {
            bail!("fee observation differs from its committed provider request");
        }
        let row = sqlx::query("SELECT r.gross_sats,r.address,r.recipient_json,g.plan_json FROM payout_rows r JOIN payout_grants g ON g.id=r.grant_id WHERE r.grant_id=? AND r.member=?")
            .bind(grant.to_string())
            .bind(&member)
            .fetch_one(&mut *tx)
            .await?;
        let gross: u64 = row.try_get::<i64, _>("gross_sats")?.try_into()?;
        let recipient: crate::payout::recipient::Recipient =
            serde_json::from_str(&row.try_get::<String, _>("recipient_json")?)?;
        recipient.validate(&row.try_get::<String, _>("address")?)?;
        let plan: Plan = serde_json::from_str(&row.try_get::<String, _>("plan_json")?)?;
        plan.planning_policy.validate()?;
        let decision = crate::payout::inclusive_fees::evaluate(
            gross,
            recipient
                .min_msat
                .div_ceil(1000)
                .max(plan.planning_policy.minimum_net_sats),
            self.policy.max_net_sats,
            doc.amount_sats,
            observation.0.fee_sats,
            observation.0.total_sats,
        )
        .map_err(anyhow::Error::msg)?;
        let json = serde_json::to_string(observation)?;
        let old: Option<(String, i64)> = sqlx::query_as(
            "SELECT document_json,round FROM payout_fee_observations WHERE provider_quote_id=?",
        )
        .bind(observation.0.quote_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((old, saved_round)) = old {
            if old != json || saved_round != i64::from(round) {
                bail!("fee observation retry changed frozen evidence");
            }
            tx.commit().await?;
            return Ok(decision);
        }
        if now < observation.0.observed_at || now >= observation.0.usable_until {
            bail!("fee observation is expired or from the future");
        }
        super::outcomes::ensure_open(&mut tx, grant, &member).await?;
        if probe_round.is_none() {
            load_doc(&mut tx, doc.id, true).await?;
        } else {
            let promoted: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM payout_invoice_intents WHERE grant_id=? AND member=?)",
            )
            .bind(grant.to_string())
            .bind(&member)
            .fetch_one(&mut *tx)
            .await?;
            if promoted {
                bail!("fee discovery is closed after executable invoice promotion");
            }
        }
        touch_clock(&mut tx, now).await?;
        sqlx::query("INSERT INTO payout_fee_observations(grant_id,member,round,provider_quote_id,gross_sats,invoice_net_sats,fee_sats,total_sats,observed_at,document_json) VALUES(?,?,?,?,?,?,?,?,?,?)")
            .bind(grant.to_string()).bind(&member)
            .bind(i64::from(round)).bind(observation.0.quote_id.to_string()).bind(i64::try_from(gross)?)
            .bind(i64::try_from(doc.amount_sats)?).bind(i64::try_from(observation.0.fee_sats)?)
            .bind(i64::try_from(observation.0.total_sats)?).bind(observation.0.observed_at).bind(json)
            .execute(&mut *tx).await?;
        if probe_round.is_none()
            && !matches!(
                &decision,
                crate::payout::inclusive_fees::Decision::Fits { .. }
            )
        {
            sqlx::query(
                "UPDATE payout_quote_intents SET retired_at=? WHERE id=? AND retired_at IS NULL",
            )
            .bind(now)
            .bind(doc.id.to_string())
            .execute(&mut *tx)
            .await?;
            stage(&mut tx, doc.id, "retired", now).await?;
        }
        tx.commit().await?;
        Ok(decision)
    }

    pub async fn record_execution_claim(
        &self,
        claim: &crate::payout::execution::AuthenticatedLease,
    ) -> Result<()> {
        let record = &claim.lease().record;
        record.validate()?;
        let request = &record.request;
        let quote = request.quote()?;
        if claim.core_uid() != self.policy.scope.core_uid
            || request.reservation.epoch != self.policy.scope.epoch
            || request.reservation.owner != self.policy.scope.owner
            || quote.request.scope != self.policy.scope.provider
        {
            bail!("execution record has wrong core/provider scope");
        }
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        let saved = load_quote(&mut tx, quote.request.id).await?;
        let row=sqlx::query("SELECT g.qualification_json,i.member FROM payout_grants g JOIN payout_invoice_intents i ON i.grant_id=g.id WHERE i.id=?")
            .bind(quote.request.invoice_id.to_string()).fetch_one(&mut *tx).await?;
        let evidence: serde_json::Value =
            serde_json::from_str(&row.try_get::<String, _>("qualification_json")?)?;
        let reservation: crate::reward_funding::Reservation =
            serde_json::from_value(evidence["reservation"].clone())?;
        if saved != quote
            || request.member != row.try_get::<String, _>("member")?
            || request.reservation != reservation
        {
            bail!(
                "core retains a different original row/quote; explicit restore reconciliation required"
            );
        }
        let json = crate::payout::execution::record_json(record)?;
        let old: Option<String> = sqlx::query_scalar(
            "SELECT document_json FROM payout_core_execution_claims WHERE grant_id=? AND member=?",
        )
        .bind(reservation.id.to_string())
        .bind(&request.member)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(old) = old {
            if old != json {
                bail!("immutable execution claim changed");
            }
        } else {
            sqlx::query("INSERT INTO payout_core_execution_claims(grant_id,member,claim_id,document_json) VALUES(?,?,?,?)")
            .bind(reservation.id.to_string()).bind(&request.member).bind(record.id.to_string()).bind(json).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn prepare_quote(&self, invoice_id: Uuid, now: i64) -> Result<QuoteIntent> {
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        super::outcomes::ensure_invoice_open(&mut tx, invoice_id).await?;
        let executed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM payout_execute_intents e JOIN payout_quote_intents q ON q.id=e.intent_id WHERE q.invoice_id=?)")
            .bind(invoice_id.to_string()).fetch_one(&mut *tx).await?;
        if executed {
            bail!("original invoice has execution intent; no fresh quote permitted");
        }
        if let Some(row)=sqlx::query("SELECT document_json FROM payout_quote_intents WHERE invoice_id=? AND retired_at IS NULL")
            .bind(invoice_id.to_string()).fetch_optional(&mut *tx).await? {
            let doc:QuoteDocument=serde_json::from_str(&row.try_get::<String,_>("document_json")?)?;
            doc.validate()?;
            if doc.amount_sats > self.policy.max_net_sats { bail!("saved quote exceeds current payment limit"); }
            tx.commit().await?;return Ok(QuoteIntent(doc));
        }
        let invoice = load_invoice(&mut tx, invoice_id, &self.policy.scope.owner)
            .await?
            .context("validated invoice is not frozen")?;
        let row=sqlx::query("SELECT r.fee_limit_sats,r.gross_sats FROM payout_rows r JOIN payout_invoice_intents i ON i.grant_id=r.grant_id AND i.member=r.member WHERE i.id=?")
            .bind(invoice_id.to_string()).fetch_one(&mut *tx).await?;
        let callback: Option<String> =
            sqlx::query_scalar("SELECT raw_zap FROM payout_callback_requests WHERE intent_id=?")
                .bind(invoice_id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        if now < 0
            || u64::try_from(now)? >= invoice.expires_at()
            || invoice.amount_sats() > self.policy.max_net_sats
        {
            bail!("saved invoice expired or exceeds current payment limit");
        }
        let doc = QuoteDocument {
            id: Uuid::new_v4(),
            invoice_id,
            scope: self.policy.scope.provider.clone(),
            encoded: invoice.encoded().into(),
            payment_hash: invoice.payment_hash().into(),
            description_hash: invoice.description_hash().into(),
            amount_sats: invoice.amount_sats(),
            fee_limit_sats: row.try_get::<i64, _>("fee_limit_sats")?.try_into()?,
            gross_sats: row.try_get::<i64, _>("gross_sats")?.try_into()?,
            invoice_expires_at: invoice.expires_at(),
            created_at: now,
            nip57: callback.is_some(),
        };
        doc.validate()?;
        touch_clock(&mut tx, now).await?;
        sqlx::query("INSERT INTO payout_quote_intents(id,invoice_id,document_json) VALUES(?,?,?)")
            .bind(doc.id.to_string())
            .bind(invoice_id.to_string())
            .bind(serde_json::to_string(&doc)?)
            .execute(&mut *tx)
            .await?;
        stage(&mut tx, doc.id, "quote_intent", now).await?;
        tx.commit().await?;
        Ok(QuoteIntent(doc))
    }

    pub async fn freeze_quote(&self, quote: &Quote, now: i64) -> Result<()> {
        quote.0.validate()?;
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        let doc = load_doc(&mut tx, quote.intent_id(), true).await?;
        if doc != quote.0.request || doc.scope != self.policy.scope.provider {
            bail!("provider quote belongs to another committed request");
        }
        let json = serde_json::to_string(&quote.0)?;
        if let Some(old) = sqlx::query_scalar::<_, String>(
            "SELECT document_json FROM payout_quote_records WHERE intent_id=?",
        )
        .bind(doc.id.to_string())
        .fetch_optional(&mut *tx)
        .await?
        {
            if old != json {
                bail!("provider quote retry changed frozen terms");
            }
            tx.commit().await?;
            return Ok(());
        }
        let duplicate: Option<String> = sqlx::query_scalar(
            "SELECT provider_quote_id FROM payout_quote_duplicates WHERE intent_id=?",
        )
        .bind(doc.id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if duplicate.is_some_and(|id| id != quote.quote_id().to_string())
            || now < quote.0.observed_at
            || now >= quote.0.usable_until
        {
            bail!("provider quote is contradictory or no longer usable");
        }
        touch_clock(&mut tx, now).await?;
        sqlx::query("INSERT INTO payout_quote_records(intent_id,provider_quote_id,document_json) VALUES(?,?,?)")
            .bind(doc.id.to_string()).bind(quote.quote_id().to_string()).bind(json).execute(&mut *tx).await?;
        stage(&mut tx, doc.id, "quote_frozen", now).await?;
        tx.commit().await?;
        Ok(())
    }

    /// A duplicate ID is a recovery observation, not fee/amount authority.
    pub async fn observe_duplicate_quote(
        &self,
        duplicate: &DuplicateQuote,
        now: i64,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        let doc = load_doc(&mut tx, duplicate.request.id, true).await?;
        if doc != duplicate.request
            || doc.scope != self.policy.scope.provider
            || duplicate.quote_id.is_nil()
        {
            bail!("duplicate quote scope differs");
        }
        let old: Option<String> = sqlx::query_scalar(
            "SELECT provider_quote_id FROM payout_quote_duplicates WHERE intent_id=?",
        )
        .bind(doc.id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(old) = old {
            if old != duplicate.quote_id.to_string() {
                bail!("duplicate quote identity changed");
            }
        } else {
            touch_clock(&mut tx, now).await?;
            sqlx::query(
                "INSERT INTO payout_quote_duplicates(intent_id,provider_quote_id) VALUES(?,?)",
            )
            .bind(doc.id.to_string())
            .bind(duplicate.quote_id.to_string())
            .execute(&mut *tx)
            .await?;
            stage(&mut tx, doc.id, "duplicate_quote", now).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn retire_quote(&self, id: Uuid, now: i64) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        let doc = load_doc(&mut tx, id, false).await?;
        let executed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM payout_execute_intents e JOIN payout_quote_intents q ON q.id=e.intent_id WHERE q.invoice_id=?)")
            .bind(doc.invoice_id.to_string()).fetch_one(&mut *tx).await?;
        if executed {
            bail!("possible execution forbids retiring or replacing its quote");
        }
        let retired: Option<i64> =
            sqlx::query_scalar("SELECT retired_at FROM payout_quote_intents WHERE id=?")
                .bind(id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        if retired.is_none() {
            touch_clock(&mut tx, now).await?;
            sqlx::query("UPDATE payout_quote_intents SET retired_at=? WHERE id=?")
                .bind(now)
                .bind(id.to_string())
                .execute(&mut *tx)
                .await?;
            stage(&mut tx, id, "retired", now).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn prepare_execute(
        &self,
        id: Uuid,
        fresh: &AuthenticatedFundingPage,
        now: i64,
    ) -> Result<ExecuteIntent> {
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        let quote = load_quote(&mut tx, id).await?;
        super::outcomes::ensure_invoice_open(&mut tx, quote.request.invoice_id).await?;
        touch_clock(&mut tx, now).await?;
        if load_binding(&mut tx, id).await?.is_some() {
            bail!("original payment ID is known; use read-only reconciliation");
        }
        let page = fresh.page();
        page.validate()?;
        let row=sqlx::query("SELECT g.*,i.member AS payout_member FROM payout_grants g JOIN payout_invoice_intents i ON i.grant_id=g.id WHERE i.id=?")
            .bind(quote.request.invoice_id.to_string()).fetch_one(&mut *tx).await?;
        let evidence: serde_json::Value =
            serde_json::from_str(&row.try_get::<String, _>("qualification_json")?)?;
        let reservation: crate::reward_funding::Reservation =
            serde_json::from_value(evidence["reservation"].clone())?;
        let funding: crate::reward_funding::Funding =
            serde_json::from_value(evidence["funding"].clone())?;
        let held = super::outcomes::usage(&mut tx, now).await?;
        let ceiling = u64::try_from(
            u128::from(funding.strike_sats) * u128::from(self.policy.member_percent_ceiling) / 100,
        )?
        .min(self.policy.max_sats_per_feed);
        if !self.policy.execution_enabled
            || fresh.core_uid() != self.policy.scope.core_uid
            || !page.execution_permitted(now)
            || page.after != 0
            || page.reservation != reservation
            || page.funding != funding
            || reservation.epoch != self.policy.scope.epoch
            || reservation.owner != self.policy.scope.owner
            || quote.request.scope != self.policy.scope.provider
            || quote.request.fee_limit_sats > self.policy.max_fee_sats
            || quote.request.amount_sats > self.policy.max_net_sats
            || reservation.reserved_sats > ceiling
            || held > self.policy.max_sats_per_24h
            || (quote.request.nip57 && !page.nostr_enabled)
        {
            bail!("fresh core and independent payout policy do not permit execution");
        }
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT document_json FROM payout_execute_intents WHERE intent_id=?",
        )
        .bind(id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let json = serde_json::to_string(&quote)?;
        let request = crate::payout::execution::Request {
            reservation,
            member: row.try_get("payout_member")?,
            quote_json: json.clone(),
        };
        let root: Option<String> = sqlx::query_scalar(
            "SELECT document_json FROM payout_core_execution_claims WHERE grant_id=? AND member=?",
        )
        .bind(request.reservation.id.to_string())
        .bind(&request.member)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(root) = &root {
            let root: crate::payout::execution::Record = serde_json::from_str(root)?;
            root.validate()?;
            if root.request != request {
                bail!("core execution record changed the original request");
            }
        }
        let recovery = existing.is_some() || root.is_some();
        if let Some(existing) = existing {
            if existing != json {
                bail!("execute retry changed original quote");
            }
        } else {
            if root.is_none()
                && (now < quote.observed_at
                    || now >= quote.usable_until
                    || u64::try_from(now)? >= quote.request.invoice_expires_at)
            {
                bail!("quote expired before first execute intent");
            }
            touch_clock(&mut tx, now).await?;
            sqlx::query("INSERT INTO payout_execute_intents(intent_id,document_json,created_at) VALUES(?,?,?)")
                .bind(id.to_string()).bind(json).bind(now).execute(&mut *tx).await?;
            stage(&mut tx, id, "execute_intent", now).await?;
        }
        tx.commit().await?;
        Ok(ExecuteIntent {
            quote,
            permitted_until: page
                .observed_at
                .checked_add(5)
                .context("execution time overflow")?,
            recovery,
            core_uid: self.policy.scope.core_uid,
            request,
        })
    }

    pub async fn bind_payment(
        &self,
        observation: &ExecutionObservation,
        now: i64,
    ) -> Result<BoundPayment> {
        let binding = &observation.0;
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        let quote = load_quote(&mut tx, binding.quote.request.id).await?;
        let executed: String = sqlx::query_scalar(
            "SELECT document_json FROM payout_execute_intents WHERE intent_id=?",
        )
        .bind(quote.request.id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if binding.quote != quote
            || quote.request.scope != self.policy.scope.provider
            || executed != serde_json::to_string(&quote)?
            || binding.payment_id.is_nil()
        {
            bail!("original execution correlation differs");
        }
        if let Some(old) = load_binding(&mut tx, quote.request.id).await? {
            if old != *binding {
                conflict(&mut tx, &old, &serde_json::to_string(binding)?, now).await?;
                tx.commit().await?;
                bail!("original quote returned conflicting payment identities");
            }
            tx.commit().await?;
            return Ok(BoundPayment(old));
        }
        touch_clock(&mut tx, now).await?;
        sqlx::query("INSERT INTO payout_payment_bindings(payment_id,intent_id,document_json,bound_at) VALUES(?,?,?,?)")
            .bind(binding.payment_id.to_string()).bind(quote.request.id.to_string()).bind(serde_json::to_string(binding)?)
            .bind(now).execute(&mut *tx).await?;
        stage(&mut tx, quote.request.id, "payment_bound", now).await?;
        tx.commit().await?;
        Ok(BoundPayment(binding.clone()))
    }

    /// Read-only recovery is available even when execution policy is disabled.
    pub async fn bound_payment(&self, id: Uuid) -> Result<Option<BoundPayment>> {
        let mut tx = self.pool.begin().await?;
        let binding = load_binding(&mut tx, id).await?;
        tx.commit().await?;
        Ok(binding.map(BoundPayment))
    }

    pub async fn observe_payment(&self, observation: &PaymentObservation, now: i64) -> Result<()> {
        let data = &observation.0;
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        let saved = load_binding(&mut tx, data.binding.quote.request.id)
            .await?
            .context("original payment not bound")?;
        if saved != data.binding
            || saved.quote.request.scope != self.policy.scope.provider
            || data.observed_at > now
            || data.observed_at < saved.quote.observed_at
            || data.fee_sats > saved.quote.fee_sats
            || saved.quote.request.amount_sats.checked_add(data.fee_sats) != Some(data.total_sats)
            || data.total_sats > saved.quote.total_sats
        {
            bail!("payment observation scope/time/amount changed");
        }
        let old:Option<String>=sqlx::query_scalar("SELECT document_json FROM payout_payment_records WHERE payment_id=? ORDER BY seq DESC LIMIT 1")
            .bind(saved.payment_id.to_string()).fetch_optional(&mut *tx).await?;
        if let Some(old) = old {
            let old: PaymentRecord = serde_json::from_str(&old)?;
            let same = old.state == data.state
                && old.fee_sats == data.fee_sats
                && old.total_sats == data.total_sats;
            if old.state != PaymentState::Pending && !same {
                conflict(&mut tx, &saved, &serde_json::to_string(data)?, now).await?;
                tx.commit().await?;
                bail!("provider contradicted a terminal payment outcome");
            }
            if same {
                tx.commit().await?;
                return Ok(());
            }
        }
        touch_clock(&mut tx, now).await?;
        sqlx::query("INSERT INTO payout_payment_records(payment_id,state,document_json,observed_at) VALUES(?,?,?,?)")
            .bind(saved.payment_id.to_string()).bind(state_name(data.state)).bind(serde_json::to_string(data)?)
            .bind(data.observed_at).execute(&mut *tx).await?;
        stage(&mut tx, saved.quote.request.id, "payment_observed", now).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn payout_status(&self, invoice_id: Uuid) -> Result<Status> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT id FROM payout_quote_intents WHERE invoice_id=? AND retired_at IS NULL",
        )
        .bind(invoice_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            return Ok(Status::Unquoted);
        };
        let id: Uuid = row.try_get::<String, _>("id")?.parse()?;
        if let Some(binding) = load_binding(&mut tx, id).await? {
            let conflict: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM payout_payment_conflicts WHERE payment_id=?)",
            )
            .bind(binding.payment_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
            if conflict {
                return Ok(Status::Conflicted);
            }
            let state:Option<String>=sqlx::query_scalar("SELECT state FROM payout_payment_records WHERE payment_id=? ORDER BY seq DESC LIMIT 1")
                .bind(binding.payment_id.to_string()).fetch_optional(&mut *tx).await?;
            return Ok(match state.as_deref() {
                Some("COMPLETED") => Status::Completed,
                Some("FAILED") => Status::Failed,
                _ => Status::Pending,
            });
        }
        let executed: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM payout_execute_intents WHERE intent_id=?)",
        )
        .bind(id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if executed {
            return Ok(Status::ExecutionUnknown);
        }
        let quoted: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM payout_quote_records WHERE intent_id=?)",
        )
        .bind(id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        Ok(if quoted {
            Status::QuoteReady
        } else {
            Status::QuoteUnknown
        })
    }
}

/// Discovery may refer to a separate probe. Never use this loader to restore
/// an executable quote; that path must retain the legacy execution-chain FKs.
async fn load_fee_doc(
    tx: &mut Transaction<'_, Sqlite>,
    id: Uuid,
    owner: &str,
) -> Result<(QuoteDocument, Uuid, String, Option<u8>)> {
    let probe: Option<(String, String, i64)> = sqlx::query_as(
        "SELECT i.grant_id,i.member,i.round FROM payout_fee_probe_quotes q JOIN payout_fee_probe_intents i ON i.id=q.invoice_id WHERE q.id=?")
        .bind(id.to_string()).fetch_optional(&mut **tx).await?;
    if let Some((grant, member, round)) = probe {
        let doc = super::fee_probes::load_quote(tx, id, owner).await?;
        return Ok((doc, grant.parse()?, member, Some(round.try_into()?)));
    }
    let doc = load_doc(tx, id, false).await?;
    let binding: (String, String) =
        sqlx::query_as("SELECT grant_id,member FROM payout_invoice_intents WHERE id=?")
            .bind(doc.invoice_id.to_string())
            .fetch_one(&mut **tx)
            .await?;
    Ok((doc, binding.0.parse()?, binding.1, None))
}

async fn load_doc(
    tx: &mut Transaction<'_, Sqlite>,
    id: Uuid,
    active: bool,
) -> Result<QuoteDocument> {
    let row = sqlx::query("SELECT * FROM payout_quote_intents WHERE id=?")
        .bind(id.to_string())
        .fetch_one(&mut **tx)
        .await?;
    let doc: QuoteDocument = serde_json::from_str(&row.try_get::<String, _>("document_json")?)?;
    doc.validate()?;
    if doc.id != id
        || doc.invoice_id.to_string() != row.try_get::<String, _>("invoice_id")?
        || (active && row.try_get::<Option<i64>, _>("retired_at")?.is_some())
    {
        bail!("original quote request is not current");
    }
    Ok(doc)
}
async fn load_quote(tx: &mut Transaction<'_, Sqlite>, id: Uuid) -> Result<QuoteRecord> {
    let doc = load_doc(tx, id, true).await?;
    let row = sqlx::query("SELECT * FROM payout_quote_records WHERE intent_id=?")
        .bind(id.to_string())
        .fetch_one(&mut **tx)
        .await?;
    let quote: QuoteRecord = serde_json::from_str(&row.try_get::<String, _>("document_json")?)?;
    quote.validate()?;
    if quote.request != doc
        || quote.quote_id.to_string() != row.try_get::<String, _>("provider_quote_id")?
    {
        bail!("saved quote terms differ");
    }
    Ok(quote)
}
async fn load_binding(tx: &mut Transaction<'_, Sqlite>, id: Uuid) -> Result<Option<BindingRecord>> {
    let Some(row) = sqlx::query("SELECT * FROM payout_payment_bindings WHERE intent_id=?")
        .bind(id.to_string())
        .fetch_optional(&mut **tx)
        .await?
    else {
        return Ok(None);
    };
    let quote = load_quote(tx, id).await?;
    let binding: BindingRecord = serde_json::from_str(&row.try_get::<String, _>("document_json")?)?;
    if binding.quote != quote
        || binding.payment_id.to_string() != row.try_get::<String, _>("payment_id")?
        || binding.payment_id.is_nil()
    {
        bail!("saved payment identity differs");
    }
    Ok(Some(binding))
}
async fn stage(tx: &mut Transaction<'_, Sqlite>, id: Uuid, name: &str, now: i64) -> Result<()> {
    sqlx::query("INSERT INTO payout_provider_journal(intent_id,stage,created_at) VALUES(?,?,?)")
        .bind(id.to_string())
        .bind(name)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
async fn conflict(
    tx: &mut Transaction<'_, Sqlite>,
    binding: &BindingRecord,
    json: &str,
    now: i64,
) -> Result<()> {
    touch_clock(tx, now).await?;
    let inserted=sqlx::query("INSERT INTO payout_payment_conflicts(payment_id,document_json,observed_at) VALUES(?,?,?) ON CONFLICT(payment_id) DO NOTHING")
        .bind(binding.payment_id.to_string()).bind(json).bind(now).execute(&mut **tx).await?;
    if inserted.rows_affected() == 1 {
        stage(tx, binding.quote.request.id, "conflict", now).await?;
    }
    Ok(())
}
fn state_name(state: PaymentState) -> &'static str {
    match state {
        PaymentState::Pending => "PENDING",
        PaymentState::Completed => "COMPLETED",
        PaymentState::Failed => "FAILED",
    }
}
