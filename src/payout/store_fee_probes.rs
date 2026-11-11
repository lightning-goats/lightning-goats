//! Pre-execution invoice identities for deducting provider fees from a fixed
//! member budget. These tables are deliberately outside the execution chain.
use super::{
    invoices::{touch_clock, write_lock},
    *,
};
use crate::payout::{
    invoice::{self, Intent, Request, ValidatedInvoice},
    provider::{QuoteDocument, QuoteIntent, restore_fee_observation},
};
use sqlx::{Sqlite, Transaction};

impl Store {
    /// Resume exactly this discovery round, or freeze its separately signed
    /// invoice identity. A changed fee can only reduce the next invoice amount.
    pub async fn prepare_fee_probe(
        &self,
        grant: Uuid,
        member: &str,
        round: u8,
        now: i64,
        joining: Option<&crate::cyberherd::conversation::PublishedReference>,
    ) -> Result<Intent> {
        crate::nostr_event::validate_public_key(member)?;
        if round > 7 {
            bail!("fee discovery round exceeds bound");
        }
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        super::outcomes::ensure_open(&mut tx, grant, member).await?;
        let old: Option<String> = sqlx::query_scalar(
            "SELECT id FROM payout_fee_probe_intents WHERE grant_id=? AND member=? AND round=?",
        )
        .bind(grant.to_string())
        .bind(member)
        .bind(i64::from(round))
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(id) = old {
            let intent = load_intent(&mut tx, id.parse()?, &self.policy.scope.owner).await?;
            tx.commit().await?;
            return Ok(intent);
        }
        let row = sqlx::query("SELECT r.*,g.admitted_at,g.plan_json FROM payout_rows r JOIN payout_grants g ON g.id=r.grant_id WHERE r.grant_id=? AND r.member=?")
            .bind(grant.to_string()).bind(member).fetch_one(&mut *tx).await?;
        let gross: u64 = row.try_get::<i64, _>("gross_sats")?.try_into()?;
        if now < row.try_get::<i64, _>("admitted_at")? || gross > self.policy.max_sats_per_feed {
            bail!("fee discovery precedes funding or exceeds current budget");
        }
        let recipient: super::super::recipient::Recipient =
            serde_json::from_str(&row.try_get::<String, _>("recipient_json")?)?;
        let plan: Plan = serde_json::from_str(&row.try_get::<String, _>("plan_json")?)?;
        plan.planning_policy.validate()?;
        let minimum = recipient
            .min_msat
            .div_ceil(1000)
            .max(plan.planning_policy.minimum_net_sats);
        let (net, previous) = if round == 0 {
            (gross, None)
        } else {
            let saved = sqlx::query("SELECT document_json,provider_quote_id FROM payout_fee_observations WHERE grant_id=? AND member=? AND round=?")
                .bind(grant.to_string()).bind(member).bind(i64::from(round)-1).fetch_one(&mut *tx).await?;
            let observation =
                restore_fee_observation(saved.try_get::<String, _>("document_json")?.as_bytes())?;
            let original =
                load_quote(&mut tx, observation.0.request.id, &self.policy.scope.owner).await?;
            let binding =
                load_intent(&mut tx, original.invoice_id, &self.policy.scope.owner).await?;
            let bound_round: i64 =
                sqlx::query_scalar("SELECT round FROM payout_fee_probe_intents WHERE id=?")
                    .bind(original.invoice_id.to_string())
                    .fetch_one(&mut *tx)
                    .await?;
            if original != observation.0.request
                || original.scope != self.policy.scope.provider
                || bound_round != i64::from(round) - 1
                || binding.grant_id() != grant
                || binding.member() != member
                || observation.0.quote_id.to_string()
                    != saved.try_get::<String, _>("provider_quote_id")?
                || original.gross_sats != gross
                || now < observation.0.observed_at
            {
                bail!("fee adjustment lost its original probe identity");
            }
            match crate::payout::inclusive_fees::evaluate(
                gross,
                minimum,
                self.policy.max_net_sats,
                original.amount_sats,
                observation.0.fee_sats,
                observation.0.total_sats,
            )
            .map_err(anyhow::Error::msg)?
            {
                crate::payout::inclusive_fees::Decision::ReduceInvoice { net } => {
                    (net, Some(observation.0.quote_id.to_string()))
                }
                _ => bail!("fee discovery cannot continue after fitting or unaffordable quote"),
            }
        };
        if net < minimum || net > self.policy.max_net_sats {
            bail!("fee probe violates delivered payment bounds");
        }
        let planned = plan
            .rows
            .iter()
            .find(|r| r.pubkey == member)
            .context("probe lost frozen member row")?;
        let intent = Intent::restore(invoice::Document {
            id: Uuid::new_v4(),
            grant_id: grant,
            owner: self.policy.scope.owner.clone(),
            member: member.into(),
            address: row.try_get("address")?,
            recipient,
            net_sats: net,
            created_at: now,
            relays: invoice::receipt_relays(&self.policy.receipt_relays)?,
            content: planned.render_zap(net)?,
            joining: joining.cloned(),
        })?;
        touch_clock(&mut tx, now).await?;
        sqlx::query("INSERT INTO payout_fee_probe_intents(id,grant_id,member,round,previous_quote_id,net_sats,document_json,created_at) VALUES(?,?,?,?,?,?,?,?)")
            .bind(intent.id().to_string()).bind(grant.to_string()).bind(member).bind(i64::from(round))
            .bind(previous).bind(i64::try_from(net)?).bind(serde_json::to_string(&intent.0)?).bind(now).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(intent)
    }

    pub async fn freeze_fee_probe_request(
        &self,
        id: Uuid,
        raw_zap: Option<&str>,
        now: i64,
    ) -> Result<Request> {
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        let intent = load_intent(&mut tx, id, &self.policy.scope.owner).await?;
        super::outcomes::ensure_open(&mut tx, intent.grant_id(), intent.member()).await?;
        if now < intent.0.created_at {
            bail!("probe callback precedes its intent");
        }
        let request = Request::restore(intent, raw_zap.map(str::to_owned))?;
        if let Some(row) =
            sqlx::query("SELECT raw_zap FROM payout_fee_probe_callbacks WHERE intent_id=?")
                .bind(id.to_string())
                .fetch_optional(&mut *tx)
                .await?
        {
            if row.try_get::<Option<String>, _>("raw_zap")?.as_deref() != raw_zap {
                bail!("probe callback retry changed signed bytes");
            }
            tx.commit().await?;
            return Ok(request);
        }
        touch_clock(&mut tx, now).await?;
        sqlx::query(
            "INSERT INTO payout_fee_probe_callbacks(intent_id,raw_zap,created_at) VALUES(?,?,?)",
        )
        .bind(id.to_string())
        .bind(raw_zap)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(request)
    }

    /// Read the committed request, including after expiry, without authorizing
    /// another callback or any provider operation.
    pub async fn fee_probe_request(&self, id: Uuid) -> Result<Request> {
        let mut tx = self.pool.begin().await?;
        let request = load_request(&mut tx, id, &self.policy.scope.owner).await?;
        tx.commit().await?;
        Ok(request)
    }

    pub async fn freeze_fee_probe_invoice(
        &self,
        invoice: &ValidatedInvoice,
        now: i64,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        let intent = load_intent(&mut tx, invoice.intent_id(), &self.policy.scope.owner).await?;
        super::outcomes::ensure_open(&mut tx, intent.grant_id(), intent.member()).await?;
        if let Some(old) =
            load_invoice(&mut tx, invoice.intent_id(), &self.policy.scope.owner).await?
        {
            if old != *invoice {
                bail!("probe retry changed its validated invoice");
            }
            tx.commit().await?;
            return Ok(());
        }
        let request = load_request(&mut tx, invoice.intent_id(), &self.policy.scope.owner).await?;
        if now < intent.0.created_at
            || request.validate_invoice(invoice.encoded(), now)? != *invoice
        {
            bail!("probe invoice differs from its committed request");
        }
        touch_clock(&mut tx, now).await?;
        sqlx::query("INSERT INTO payout_fee_probe_invoices(intent_id,payment_hash,document_json,validated_at) VALUES(?,?,?,?)")
            .bind(invoice.intent_id().to_string()).bind(invoice.payment_hash()).bind(serde_json::to_string(invoice)?).bind(now)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn prepare_fee_probe_quote(&self, id: Uuid, now: i64) -> Result<QuoteIntent> {
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        let intent = load_intent(&mut tx, id, &self.policy.scope.owner).await?;
        super::outcomes::ensure_open(&mut tx, intent.grant_id(), intent.member()).await?;
        if let Some(old) = sqlx::query_scalar::<_, String>(
            "SELECT id FROM payout_fee_probe_quotes WHERE invoice_id=?",
        )
        .bind(id.to_string())
        .fetch_optional(&mut *tx)
        .await?
        {
            let doc = load_quote(&mut tx, old.parse()?, &self.policy.scope.owner).await?;
            if doc.scope != self.policy.scope.provider {
                bail!("probe provider scope changed");
            }
            tx.commit().await?;
            return Ok(QuoteIntent(doc));
        }
        let invoice = load_invoice(&mut tx, id, &self.policy.scope.owner)
            .await?
            .context("probe invoice not frozen")?;
        let gross: i64 =
            sqlx::query_scalar("SELECT gross_sats FROM payout_rows WHERE grant_id=? AND member=?")
                .bind(intent.grant_id().to_string())
                .bind(intent.member())
                .fetch_one(&mut *tx)
                .await?;
        let gross = u64::try_from(gross)?;
        if now < intent.0.created_at
            || u64::try_from(now)? >= invoice.expires_at()
            || invoice.amount_sats() > self.policy.max_net_sats
            || gross > self.policy.max_sats_per_feed
        {
            bail!("probe invoice expired or exceeds current budget");
        }
        let doc = QuoteDocument {
            id: Uuid::new_v4(),
            invoice_id: id,
            scope: self.policy.scope.provider.clone(),
            encoded: invoice.encoded().into(),
            payment_hash: invoice.payment_hash().into(),
            description_hash: invoice.description_hash().into(),
            amount_sats: invoice.amount_sats(),
            fee_limit_sats: gross
                .checked_sub(invoice.amount_sats())
                .context("probe exceeds original gross")?,
            gross_sats: gross,
            invoice_expires_at: invoice.expires_at(),
            created_at: now,
            nip57: intent.uses_nostr(),
        };
        doc.validate()?;
        touch_clock(&mut tx, now).await?;
        sqlx::query(
            "INSERT INTO payout_fee_probe_quotes(id,invoice_id,document_json) VALUES(?,?,?)",
        )
        .bind(doc.id.to_string())
        .bind(id.to_string())
        .bind(serde_json::to_string(&doc)?)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(QuoteIntent(doc))
    }
}

async fn load_intent(tx: &mut Transaction<'_, Sqlite>, id: Uuid, owner: &str) -> Result<Intent> {
    let row = sqlx::query("SELECT * FROM payout_fee_probe_intents WHERE id=?")
        .bind(id.to_string())
        .fetch_one(&mut **tx)
        .await?;
    let doc: invoice::Document = serde_json::from_str(&row.try_get::<String, _>("document_json")?)?;
    if doc.id != id
        || doc.owner != owner
        || doc.grant_id.to_string() != row.try_get::<String, _>("grant_id")?
        || doc.member != row.try_get::<String, _>("member")?
        || doc.net_sats != u64::try_from(row.try_get::<i64, _>("net_sats")?)?
        || doc.created_at != row.try_get::<i64, _>("created_at")?
    {
        bail!("probe intent binding differs");
    }
    Intent::restore(doc)
}

async fn load_request(tx: &mut Transaction<'_, Sqlite>, id: Uuid, owner: &str) -> Result<Request> {
    let intent = load_intent(tx, id, owner).await?;
    let row =
        sqlx::query("SELECT raw_zap,created_at FROM payout_fee_probe_callbacks WHERE intent_id=?")
            .bind(id.to_string())
            .fetch_one(&mut **tx)
            .await?;
    if row.try_get::<i64, _>("created_at")? < intent.0.created_at {
        bail!("probe callback precedes its intent");
    }
    Request::restore(intent, row.try_get("raw_zap")?)
}

async fn load_invoice(
    tx: &mut Transaction<'_, Sqlite>,
    id: Uuid,
    owner: &str,
) -> Result<Option<ValidatedInvoice>> {
    let Some(row) = sqlx::query("SELECT * FROM payout_fee_probe_invoices WHERE intent_id=?")
        .bind(id.to_string())
        .fetch_optional(&mut **tx)
        .await?
    else {
        return Ok(None);
    };
    let raw: String = row.try_get("document_json")?;
    // ValidatedInvoice intentionally has no Deserialize implementation. Restore
    // only by re-verifying the original BOLT11 against its frozen signed request.
    let json: serde_json::Value = serde_json::from_str(&raw)?;
    let encoded = json
        .get("encoded")
        .and_then(|v| v.as_str())
        .context("probe invoice lacks encoding")?;
    let invoice = load_request(tx, id, owner)
        .await?
        .validate_invoice(encoded, row.try_get("validated_at")?)?;
    if invoice.payment_hash() != row.try_get::<String, _>("payment_hash")?
        || serde_json::to_string(&invoice)? != raw
    {
        bail!("saved probe invoice contradicts frozen request");
    }
    Ok(Some(invoice))
}

pub(super) async fn load_quote(
    tx: &mut Transaction<'_, Sqlite>,
    id: Uuid,
    owner: &str,
) -> Result<QuoteDocument> {
    let row = sqlx::query("SELECT * FROM payout_fee_probe_quotes WHERE id=?")
        .bind(id.to_string())
        .fetch_one(&mut **tx)
        .await?;
    let doc: QuoteDocument = serde_json::from_str(&row.try_get::<String, _>("document_json")?)?;
    doc.validate()?;
    let invoice = load_invoice(tx, doc.invoice_id, owner)
        .await?
        .context("probe quote lost invoice")?;
    let intent = load_intent(tx, doc.invoice_id, owner).await?;
    let gross: i64 =
        sqlx::query_scalar("SELECT gross_sats FROM payout_rows WHERE grant_id=? AND member=?")
            .bind(intent.grant_id().to_string())
            .bind(intent.member())
            .fetch_one(&mut **tx)
            .await?;
    if doc.id != id
        || doc.invoice_id.to_string() != row.try_get::<String, _>("invoice_id")?
        || doc.gross_sats != u64::try_from(gross)?
        || doc.amount_sats != invoice.amount_sats()
        || doc.encoded != invoice.encoded()
        || doc.payment_hash != invoice.payment_hash()
        || doc.description_hash != invoice.description_hash()
        || doc.invoice_expires_at != invoice.expires_at()
        || doc.nip57 != intent.uses_nostr()
        || doc.created_at < intent.0.created_at
    {
        bail!("probe quote contradicts original invoice");
    }
    Ok(doc)
}

impl Store {
    pub async fn grant_planning_policy(
        &self,
        grant: Uuid,
    ) -> Result<crate::cyberherd::rewards::PlanningPolicy> {
        let raw: String = sqlx::query_scalar("SELECT plan_json FROM payout_grants WHERE id=?")
            .bind(grant.to_string())
            .fetch_one(&self.pool)
            .await?;
        let plan: Plan = serde_json::from_str(&raw)?;
        plan.planning_policy.validate()?;
        Ok(plan.planning_policy)
    }
    pub async fn grant_fee_mode(&self, grant: Uuid) -> Result<crate::payout::inclusive_fees::Mode> {
        Ok(self.grant_planning_policy(grant).await?.fee_mode)
    }
    pub async fn latest_fee_probe(
        &self,
        grant: Uuid,
        member: &str,
    ) -> Result<Option<(u8, Intent)>> {
        let mut tx = self.pool.begin().await?;
        let row: Option<(String,i64)> = sqlx::query_as("SELECT id,round FROM payout_fee_probe_intents WHERE grant_id=? AND member=? ORDER BY round DESC LIMIT 1")
            .bind(grant.to_string()).bind(member).fetch_optional(&mut *tx).await?;
        let result = match row {
            Some((id, round)) => Some((
                round.try_into()?,
                load_intent(&mut tx, id.parse()?, &self.policy.scope.owner).await?,
            )),
            None => None,
        };
        tx.commit().await?;
        Ok(result)
    }
    pub async fn fee_probe_callback(&self, id: Uuid) -> Result<Option<Request>> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM payout_fee_probe_callbacks WHERE intent_id=?)",
        )
        .bind(id.to_string())
        .fetch_one(&self.pool)
        .await?;
        if exists {
            Ok(Some(self.fee_probe_request(id).await?))
        } else {
            Ok(None)
        }
    }
    pub async fn saved_fee_probe_invoice(&self, id: Uuid) -> Result<Option<ValidatedInvoice>> {
        let mut tx = self.pool.begin().await?;
        let invoice = load_invoice(&mut tx, id, &self.policy.scope.owner).await?;
        tx.commit().await?;
        Ok(invoice)
    }

    /// Copy the exact final probe into the existing single execution chain in
    /// one transaction. This creates no provider request or payment authority.
    pub async fn promote_fee_probe(
        &self,
        grant: Uuid,
        member: &str,
        now: i64,
    ) -> Result<QuoteIntent> {
        if self.policy.fee_mode.is_fixed() {
            bail!("inclusive fee promotion is not selected");
        }
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        super::outcomes::ensure_open(&mut tx, grant, member).await?;
        let row = sqlx::query("SELECT o.*,g.plan_json FROM payout_fee_observations o JOIN payout_grants g ON g.id=o.grant_id WHERE o.grant_id=? AND o.member=? ORDER BY o.round DESC LIMIT 1")
            .bind(grant.to_string()).bind(member).fetch_one(&mut *tx).await?;
        let observation =
            restore_fee_observation(row.try_get::<String, _>("document_json")?.as_bytes())?;
        let doc = load_quote(&mut tx, observation.0.request.id, &self.policy.scope.owner).await?;
        let intent = load_intent(&mut tx, doc.invoice_id, &self.policy.scope.owner).await?;
        let plan: Plan = serde_json::from_str(&row.try_get::<String, _>("plan_json")?)?;
        plan.planning_policy.validate()?;
        let probe_round: i64 =
            sqlx::query_scalar("SELECT round FROM payout_fee_probe_intents WHERE id=?")
                .bind(intent.id().to_string())
                .fetch_one(&mut *tx)
                .await?;
        let planned = plan
            .rows
            .iter()
            .find(|r| r.pubkey == member)
            .context("promotion lost frozen member")?;
        let recipient = planned
            .recipient
            .as_ref()
            .context("promotion lost recipient")?;
        if plan.planning_policy.fee_mode != self.policy.fee_mode
            || intent.grant_id() != grant
            || intent.member() != member
            || doc != observation.0.request
            || doc.scope != self.policy.scope.provider
            || probe_round != row.try_get::<i64, _>("round")?
            || observation.0.quote_id.to_string()
                != row.try_get::<String, _>("provider_quote_id")?
            || doc.gross_sats != planned.gross_sats
            || doc.amount_sats != intent.0.net_sats
            || doc.gross_sats != u64::try_from(row.try_get::<i64, _>("gross_sats")?)?
            || doc.amount_sats != u64::try_from(row.try_get::<i64, _>("invoice_net_sats")?)?
            || observation.0.fee_sats != u64::try_from(row.try_get::<i64, _>("fee_sats")?)?
            || observation.0.total_sats != u64::try_from(row.try_get::<i64, _>("total_sats")?)?
            || observation.0.observed_at != row.try_get::<i64, _>("observed_at")?
        {
            bail!("fee promotion differs from original plan and discovery evidence");
        }
        let decision = crate::payout::inclusive_fees::evaluate(
            doc.gross_sats,
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
        if !matches!(
            decision,
            crate::payout::inclusive_fees::Decision::Fits { .. }
        ) {
            bail!("oversized/unaffordable fee probe cannot be promoted");
        }
        let quote = observation.executable_quote()?;
        if let Some(old) =
            sqlx::query("SELECT * FROM payout_fee_promotions WHERE grant_id=? AND member=?")
                .bind(grant.to_string())
                .bind(member)
                .fetch_optional(&mut *tx)
                .await?
        {
            if old.try_get::<String, _>("invoice_id")? != intent.id().to_string()
                || old.try_get::<String, _>("quote_id")? != doc.id.to_string()
                || old.try_get::<String, _>("provider_quote_id")? != quote.quote_id().to_string()
            {
                bail!("fee promotion retry changed identities");
            }
            tx.commit().await?;
            return Ok(QuoteIntent(doc));
        }
        if now < observation.0.observed_at
            || now >= observation.0.usable_until
            || doc.gross_sats > self.policy.max_sats_per_feed
        {
            bail!("fee promotion expired or exceeds current budget");
        }
        let invoice = load_invoice(&mut tx, intent.id(), &self.policy.scope.owner)
            .await?
            .context("promotion lost invoice")?;
        let callback = sqlx::query(
            "SELECT raw_zap,created_at FROM payout_fee_probe_callbacks WHERE intent_id=?",
        )
        .bind(intent.id().to_string())
        .fetch_one(&mut *tx)
        .await?;
        let raw_zap: Option<String> = callback.try_get("raw_zap")?;
        let validated_at: i64 = sqlx::query_scalar(
            "SELECT validated_at FROM payout_fee_probe_invoices WHERE intent_id=?",
        )
        .bind(intent.id().to_string())
        .fetch_one(&mut *tx)
        .await?;
        touch_clock(&mut tx, now).await?;
        sqlx::query(
            "INSERT INTO payout_invoice_intents(id,grant_id,member,document_json) VALUES(?,?,?,?)",
        )
        .bind(intent.id().to_string())
        .bind(grant.to_string())
        .bind(member)
        .bind(serde_json::to_string(&intent.0)?)
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO payout_callback_requests(intent_id,protocol,raw_zap,created_at) VALUES(?,?,?,?)")
            .bind(intent.id().to_string()).bind(if raw_zap.is_some(){"nip57"}else{"ordinary"}).bind(raw_zap)
            .bind(callback.try_get::<i64,_>("created_at")?).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO payout_invoices(intent_id,payment_hash,encoded,document_json,validated_at) VALUES(?,?,?,?,?)")
            .bind(intent.id().to_string()).bind(invoice.payment_hash()).bind(invoice.encoded())
            .bind(serde_json::to_string(&invoice)?).bind(validated_at).execute(&mut *tx).await?;
        for stage in ["intent", "callback", "invoice"] {
            sqlx::query(
                "INSERT INTO payout_invoice_journal(intent_id,stage,created_at) VALUES(?,?,?)",
            )
            .bind(intent.id().to_string())
            .bind(stage)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("INSERT INTO payout_quote_intents(id,invoice_id,document_json) VALUES(?,?,?)")
            .bind(doc.id.to_string())
            .bind(intent.id().to_string())
            .bind(serde_json::to_string(&doc)?)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO payout_quote_records(intent_id,provider_quote_id,document_json) VALUES(?,?,?)")
            .bind(doc.id.to_string()).bind(quote.quote_id().to_string()).bind(serde_json::to_string(&quote.0)?).execute(&mut *tx).await?;
        for stage in ["quote_intent", "quote_frozen"] {
            sqlx::query(
                "INSERT INTO payout_provider_journal(intent_id,stage,created_at) VALUES(?,?,?)",
            )
            .bind(doc.id.to_string())
            .bind(stage)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("INSERT INTO payout_fee_promotions(grant_id,member,invoice_id,quote_id,provider_quote_id,created_at) VALUES(?,?,?,?,?,?)")
            .bind(grant.to_string()).bind(member).bind(intent.id().to_string()).bind(doc.id.to_string()).bind(quote.quote_id().to_string()).bind(now).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(QuoteIntent(doc))
    }

    pub async fn record_fee_probe_duplicate(
        &self,
        duplicate: &crate::payout::provider::DuplicateQuote,
        now: i64,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        let doc = load_quote(&mut tx, duplicate.request.id, &self.policy.scope.owner).await?;
        if doc != duplicate.request
            || doc.scope != self.policy.scope.provider
            || duplicate.quote_id.is_nil()
            || now < doc.created_at
        {
            bail!("duplicate fee quote differs from original request");
        }
        let old: Option<String> = sqlx::query_scalar(
            "SELECT provider_quote_id FROM payout_fee_probe_duplicates WHERE quote_id=?",
        )
        .bind(doc.id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(old) = old {
            if old != duplicate.quote_id.to_string() {
                bail!("duplicate fee quote changed original identity");
            }
        } else {
            touch_clock(&mut tx, now).await?;
            sqlx::query("INSERT INTO payout_fee_probe_duplicates(quote_id,provider_quote_id,created_at) VALUES(?,?,?)")
                .bind(doc.id.to_string()).bind(duplicate.quote_id.to_string()).bind(now).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
