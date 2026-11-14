//! Freeze each outgoing boundary before an external signer or callback sees it.
use super::*;
use crate::payout::invoice::{self, Intent, Request, ValidatedInvoice};
use sqlx::{Sqlite, Transaction};

impl Store {
    pub async fn prepare_invoice(&self, grant_id: Uuid, member: &str, now: i64) -> Result<Intent> {
        self.prepare_invoice_with_reference(grant_id, member, now, None)
            .await
    }
    pub(crate) async fn prepare_invoice_with_reference(
        &self,
        grant_id: Uuid,
        member: &str,
        now: i64,
        joining: Option<&crate::cyberherd::conversation::PublishedReference>,
    ) -> Result<Intent> {
        crate::nostr_event::validate_public_key(member)?;
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        super::outcomes::ensure_open(&mut tx, grant_id, member).await?;
        if let Some(row) =
            sqlx::query("SELECT id FROM payout_invoice_intents WHERE grant_id=? AND member=?")
                .bind(grant_id.to_string())
                .bind(member)
                .fetch_optional(&mut *tx)
                .await?
        {
            let intent = load_intent(
                &mut tx,
                &row.try_get::<String, _>("id")?,
                &self.policy.scope.owner,
            )
            .await?;
            tx.commit().await?;
            return Ok(intent);
        }
        let row=sqlx::query("SELECT r.*,g.admitted_at,g.plan_json FROM payout_rows r JOIN payout_grants g ON g.id=r.grant_id WHERE r.grant_id=? AND r.member=?")
            .bind(grant_id.to_string()).bind(member).fetch_one(&mut *tx).await?;
        if now < row.try_get::<i64, _>("admitted_at")?
            || u64::try_from(row.try_get::<i64, _>("net_sats")?)? > self.policy.max_net_sats
            || u64::try_from(row.try_get::<i64, _>("fee_limit_sats")?)? > self.policy.max_fee_sats
        {
            bail!("invoice preparation precedes funding or exceeds current amount/fee policy");
        }
        let plan: crate::cyberherd::rewards::Plan =
            serde_json::from_str(&row.try_get::<String, _>("plan_json")?)?;
        if !plan.planning_policy.fee_mode.is_fixed() || !self.policy.fee_mode.is_fixed() {
            bail!("inclusive payouts require their separate probe and promotion path");
        }
        let planned = plan
            .rows
            .iter()
            .find(|r| r.pubkey == member)
            .context("invoice lost its frozen plan row")?;
        let intent = Intent::restore(invoice::Document {
            id: Uuid::new_v4(),
            grant_id,
            owner: self.policy.scope.owner.clone(),
            member: member.into(),
            address: row.try_get("address")?,
            recipient: serde_json::from_str(&row.try_get::<String, _>("recipient_json")?)?,
            net_sats: row.try_get::<i64, _>("net_sats")?.try_into()?,
            created_at: now,
            relays: invoice::receipt_relays(&self.policy.receipt_relays)?,
            content: planned.zap_content.clone().unwrap_or_default(),
            joining: joining.cloned(),
        })?;
        touch_clock(&mut tx, now).await?;
        sqlx::query(
            "INSERT INTO payout_invoice_intents(id,grant_id,member,document_json) VALUES(?,?,?,?)",
        )
        .bind(intent.id().to_string())
        .bind(grant_id.to_string())
        .bind(member)
        .bind(serde_json::to_string(&intent.0)?)
        .execute(&mut *tx)
        .await?;
        invoice_stage(&mut tx, intent.id(), "intent", now).await?;
        tx.commit().await?;
        Ok(intent)
    }

    pub async fn freeze_request(
        &self,
        intent_id: Uuid,
        raw_zap: Option<&str>,
        now: i64,
    ) -> Result<Request> {
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        let intent = load_intent(&mut tx, &intent_id.to_string(), &self.policy.scope.owner).await?;
        if now < intent.0.created_at {
            bail!("callback freeze precedes invoice intent");
        }
        let request = Request::restore(intent, raw_zap.map(str::to_owned))?;
        if let Some(row) =
            sqlx::query("SELECT raw_zap FROM payout_callback_requests WHERE intent_id=?")
                .bind(intent_id.to_string())
                .fetch_optional(&mut *tx)
                .await?
        {
            if row.try_get::<Option<String>, _>("raw_zap")?.as_deref() != raw_zap {
                bail!("callback retry changed the frozen zap bytes");
            }
            tx.commit().await?;
            return Ok(request);
        }
        touch_clock(&mut tx, now).await?;
        sqlx::query("INSERT INTO payout_callback_requests(intent_id,protocol,raw_zap,created_at) VALUES(?,?,?,?)")
            .bind(intent_id.to_string()).bind(if raw_zap.is_some(){"nip57"}else{"ordinary"})
            .bind(raw_zap).bind(now).execute(&mut *tx).await?;
        invoice_stage(&mut tx, intent_id, "callback", now).await?;
        tx.commit().await?;
        Ok(request)
    }

    /// Recover the same committed callback. No signing or invoice replacement
    /// happens implicitly. A saved invoice closes further callback work.
    pub async fn invoice_request(&self, intent_id: Uuid) -> Result<Request> {
        let mut tx = self.pool.begin().await?;
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM payout_invoices WHERE intent_id=?)")
                .bind(intent_id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        if exists {
            bail!("outgoing invoice already frozen");
        }
        let request =
            load_request(&mut tx, &intent_id.to_string(), &self.policy.scope.owner).await?;
        tx.commit().await?;
        Ok(request)
    }

    pub async fn freeze_invoice(
        &self,
        invoice: &ValidatedInvoice,
        now: i64,
    ) -> Result<ValidatedInvoice> {
        let id = invoice.intent_id();
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        if let Some(old) = load_invoice(&mut tx, id, &self.policy.scope.owner).await? {
            if old != *invoice {
                bail!("invoice retry changed the saved payment identity");
            }
            tx.commit().await?;
            return Ok(old);
        }
        let request = load_request(&mut tx, &id.to_string(), &self.policy.scope.owner).await?;
        let verified = request.validate_invoice(invoice.encoded(), now)?;
        if verified != *invoice {
            bail!("outgoing invoice differs from frozen callback intent");
        }
        touch_clock(&mut tx, now).await?;
        sqlx::query("INSERT INTO payout_invoices(intent_id,payment_hash,encoded,document_json,validated_at) VALUES(?,?,?,?,?)")
            .bind(id.to_string()).bind(invoice.payment_hash()).bind(invoice.encoded())
            .bind(serde_json::to_string(invoice)?).bind(now).execute(&mut *tx).await?;
        invoice_stage(&mut tx, id, "invoice", now).await?;
        tx.commit().await?;
        Ok(verified)
    }

    /// Historical validation remains readable after expiry for reconciliation.
    /// Quote/execute must independently check current expiry and policy later.
    pub async fn saved_invoice(&self, intent_id: Uuid) -> Result<Option<ValidatedInvoice>> {
        let mut tx = self.pool.begin().await?;
        let invoice = load_invoice(&mut tx, intent_id, &self.policy.scope.owner).await?;
        tx.commit().await?;
        Ok(invoice)
    }
}

pub(super) async fn write_lock(tx: &mut Transaction<'_, Sqlite>) -> Result<()> {
    sqlx::query("UPDATE payout_clock SET observed_at=observed_at WHERE singleton=1")
        .execute(&mut **tx)
        .await?;
    Ok(())
}
pub(super) async fn touch_clock(tx: &mut Transaction<'_, Sqlite>, now: i64) -> Result<()> {
    let previous: i64 =
        sqlx::query_scalar("SELECT observed_at FROM payout_clock WHERE singleton=1")
            .fetch_one(&mut **tx)
            .await?;
    if now < previous {
        bail!("payout clock moved backwards");
    }
    sqlx::query("UPDATE payout_clock SET observed_at=? WHERE singleton=1")
        .bind(now)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
async fn invoice_stage(
    tx: &mut Transaction<'_, Sqlite>,
    id: Uuid,
    stage: &str,
    now: i64,
) -> Result<()> {
    sqlx::query("INSERT INTO payout_invoice_journal(intent_id,stage,created_at) VALUES(?,?,?)")
        .bind(id.to_string())
        .bind(stage)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
pub(super) async fn load_intent(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    owner: &str,
) -> Result<Intent> {
    let row = sqlx::query("SELECT * FROM payout_invoice_intents WHERE id=?")
        .bind(id)
        .fetch_one(&mut **tx)
        .await?;
    let document: invoice::Document =
        serde_json::from_str(&row.try_get::<String, _>("document_json")?)?;
    if document.id.to_string() != id
        || document.owner != owner
        || document.grant_id.to_string() != row.try_get::<String, _>("grant_id")?
        || document.member != row.try_get::<String, _>("member")?
    {
        bail!("stored outgoing invoice scope differs");
    }
    Intent::restore(document)
}
async fn load_request(tx: &mut Transaction<'_, Sqlite>, id: &str, owner: &str) -> Result<Request> {
    let intent = load_intent(tx, id, owner).await?;
    let row =
        sqlx::query("SELECT protocol,raw_zap FROM payout_callback_requests WHERE intent_id=?")
            .bind(id)
            .fetch_one(&mut **tx)
            .await?;
    let raw: Option<String> = row.try_get("raw_zap")?;
    if row.try_get::<String, _>("protocol")? != if raw.is_some() { "nip57" } else { "ordinary" } {
        bail!("stored callback protocol differs");
    }
    Request::restore(intent, raw)
}
pub(super) async fn load_invoice(
    tx: &mut Transaction<'_, Sqlite>,
    id: Uuid,
    owner: &str,
) -> Result<Option<ValidatedInvoice>> {
    let Some(row) = sqlx::query("SELECT * FROM payout_invoices WHERE intent_id=?")
        .bind(id.to_string())
        .fetch_optional(&mut **tx)
        .await?
    else {
        return Ok(None);
    };
    let request = load_request(tx, &id.to_string(), owner).await?;
    let invoice = request.validate_invoice(
        &row.try_get::<String, _>("encoded")?,
        row.try_get("validated_at")?,
    )?;
    if invoice.payment_hash() != row.try_get::<String, _>("payment_hash")?
        || serde_json::to_string(&invoice)? != row.try_get::<String, _>("document_json")?
    {
        bail!("stored invoice contradicts its signed request");
    }
    Ok(Some(invoice))
}
