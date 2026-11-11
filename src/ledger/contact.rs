//! Separate fee-purpose state. No public event or feed/reward mutation.
use anyhow::{Context, Result, bail};
use sqlx::Row as _;
use uuid::Uuid;

use super::{LedgerStore, StoredStrikeReceiveRequest};
use crate::contact::{FEE_MSAT, Intent, Status, capability_hash};

pub struct ContactDelivery {
    pub contact_id: Uuid,
    pub lease_id: Uuid,
    pub envelope_json: String,
    pub recipient: String,
    pub relays: Vec<String>,
}

impl LedgerStore {
    pub async fn create_contact_intent(&self, intent: &Intent) -> Result<()> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(row) = sqlx::query("SELECT capability_hash,reply_email_hash,envelope_id,envelope_json,recipient,description_hash,relays_json FROM private_contact_intents WHERE id=?")
            .bind(intent.id.to_string()).fetch_optional(&mut *tx).await? {
            for (key, expected) in [
                ("capability_hash", &intent.capability_hash), ("reply_email_hash", &intent.reply_email_hash),
                ("envelope_id", &intent.envelope_id), ("envelope_json", &intent.envelope_json),
                ("recipient", &intent.recipient), ("description_hash", &intent.description_hash),
            ] {
                if row.try_get::<String,_>(key)? != *expected { bail!("conflicting immutable contact intent"); }
            }
            tx.commit().await?;
            return Ok(());
        }
        let counts: (i64,i64) = sqlx::query_as("SELECT COUNT(*),COALESCE(SUM(CASE WHEN created_at>unixepoch()-86400 THEN 1 ELSE 0 END),0) FROM private_contact_intents")
            .fetch_one(&mut *tx).await?;
        if counts.0 >= 10_000 || counts.1 >= 1000 {
            bail!("private contact admission exhausted");
        }
        sqlx::query("INSERT INTO private_contact_intents(id,capability_hash,reply_email_hash,envelope_id,envelope_json,recipient,description_hash,relays_json) VALUES(?,?,?,?,?,?,?,?)")
            .bind(intent.id.to_string()).bind(&intent.capability_hash).bind(&intent.reply_email_hash)
            .bind(&intent.envelope_id).bind(&intent.envelope_json).bind(&intent.recipient)
            .bind(&intent.description_hash).bind(&intent.relays_json).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Never expires into another provider creation: an interrupted claim is an
    /// unknown create outcome, not authority to create a replacement invoice.
    pub async fn claim_contact_issuance(&self, id: Uuid, capability: &str) -> Result<Option<Uuid>> {
        let hash = capability_hash(capability)?;
        let claim = Uuid::new_v4();
        let updated=sqlx::query("UPDATE private_contact_intents SET state='issuing',issuance_claim=? WHERE id=? AND capability_hash=? AND state='new'")
            .bind(claim.to_string()).bind(id.to_string()).bind(hash).execute(&self.pool).await?;
        Ok((updated.rows_affected() == 1).then_some(claim))
    }

    pub async fn mark_contact_issuance_uncertain(&self, id: Uuid, claim: Uuid) -> Result<()> {
        sqlx::query("UPDATE private_contact_intents SET state='uncertain' WHERE id=? AND issuance_claim=? AND state='issuing'")
            .bind(id.to_string()).bind(claim.to_string()).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn contact_status(&self, id: Uuid, capability: &str) -> Result<Option<Status>> {
        let hash = capability_hash(capability)?;
        let row=sqlx::query("SELECT i.state,r.invoice,f.contact_id AS paid,d.accepted_at FROM private_contact_intents i LEFT JOIN strike_receive_requests r ON r.receive_request_id=i.receive_request_id LEFT JOIN private_contact_fees f ON f.contact_id=i.id LEFT JOIN private_contact_delivery d ON d.contact_id=i.id WHERE i.id=? AND i.capability_hash=?")
            .bind(id.to_string()).bind(hash).fetch_optional(&self.pool).await?;
        row.map(|row| {
            let state = if row.try_get::<Option<i64>, _>("accepted_at")?.is_some() {
                "delivered".to_owned()
            } else if row.try_get::<Option<String>, _>("paid")?.is_some() {
                "paid".to_owned()
            } else {
                row.try_get("state")?
            };
            Ok(Status {
                id,
                state,
                fee_sats: FEE_MSAT / 1000,
                invoice: row.try_get("invoice")?,
            })
        })
        .transpose()
    }

    pub(crate) async fn contact_for_receive(&self, id: Uuid) -> Result<Option<Uuid>> {
        sqlx::query_scalar::<_, String>(
            "SELECT id FROM private_contact_intents WHERE receive_request_id=? AND state='issued'",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .map(|id| Uuid::parse_str(&id).map_err(Into::into))
        .transpose()
    }

    pub(crate) async fn record_contact_invoice(
        &self,
        id: Uuid,
        claim: Uuid,
        request: &StoredStrikeReceiveRequest,
    ) -> Result<()> {
        if request.address_user != "contact"
            || request.credit_pool != "contact"
            || request.amount_msat != FEE_MSAT
        {
            bail!("contact invoice has the wrong accounting purpose or amount");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row=sqlx::query("SELECT state,issuance_claim,description_hash,receive_request_id FROM private_contact_intents WHERE id=?")
            .bind(id.to_string()).fetch_one(&mut *tx).await?;
        if row.try_get::<String, _>("description_hash")? != request.description_hash
            || row
                .try_get::<Option<String>, _>("issuance_claim")?
                .as_deref()
                != Some(&claim.to_string())
        {
            bail!("contact invoice differs from its immutable intent or claim");
        }
        let state: String = row.try_get("state")?;
        if !["issuing", "uncertain", "issued"].contains(&state.as_str()) {
            bail!("contact issuance is not owned");
        }
        if state == "issued"
            && row
                .try_get::<Option<String>, _>("receive_request_id")?
                .as_deref()
                != Some(&request.receive_request_id.to_string())
        {
            bail!("contact already owns another invoice");
        }
        let collision: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM settled_payments WHERE payment_hash=? COLLATE NOCASE)",
        )
        .bind(&request.payment_hash)
        .fetch_one(&mut *tx)
        .await?;
        if collision {
            bail!("private contact invoice collides with an ordinary payment");
        }
        super::strike::record_in_transaction(&mut tx, request, None).await?;
        sqlx::query(
            "UPDATE private_contact_intents SET state='issued',receive_request_id=? WHERE id=?",
        )
        .bind(request.receive_request_id.to_string())
        .bind(id.to_string())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn accept_contact_fee(
        &self,
        contact: Uuid,
        request: Uuid,
        receive: Uuid,
        amount_msat: u64,
        invoice_hash: &str,
    ) -> Result<bool> {
        if amount_msat != FEE_MSAT {
            bail!("contact fee must be exactly 100 sats");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let stored=sqlx::query("SELECT r.payment_hash,r.amount_msat FROM private_contact_intents i JOIN strike_receive_requests r ON r.receive_request_id=i.receive_request_id WHERE i.id=? AND i.receive_request_id=? AND i.state='issued'")
            .bind(contact.to_string()).bind(request.to_string()).fetch_one(&mut *tx).await?;
        if stored.try_get::<String, _>("payment_hash")? != invoice_hash
            || stored.try_get::<i64, _>("amount_msat")? != i64::try_from(FEE_MSAT)?
        {
            bail!("contact settlement differs from the issued invoice");
        }
        let collision:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM settled_payments WHERE (source='strike' AND source_id=?) OR payment_hash=? COLLATE NOCASE)")
            .bind(receive.to_string()).bind(invoice_hash).fetch_one(&mut *tx).await?;
        if collision {
            bail!("private fee collides with an ordinary settlement");
        }
        if let Some(row)=sqlx::query("SELECT receive_id,invoice_payment_hash,amount_msat FROM private_contact_fees WHERE contact_id=?")
            .bind(contact.to_string()).fetch_optional(&mut *tx).await? {
            if row.try_get::<String,_>("receive_id")?!=receive.to_string() || row.try_get::<String,_>("invoice_payment_hash")?!=invoice_hash || row.try_get::<i64,_>("amount_msat")?!=i64::try_from(amount_msat)? {bail!("conflicting private fee settlement");}
            tx.commit().await?;return Ok(false);
        }
        sqlx::query("INSERT INTO private_contact_fees(contact_id,receive_id,invoice_payment_hash,amount_msat) VALUES(?,?,?,?)")
            .bind(contact.to_string()).bind(receive.to_string()).bind(invoice_hash).bind(i64::try_from(amount_msat)?).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO private_contact_delivery(contact_id) VALUES(?)")
            .bind(contact.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn reserve_contact_delivery(&self) -> Result<Option<ContactDelivery>> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row=sqlx::query("SELECT i.id,i.envelope_json,i.recipient,i.relays_json FROM private_contact_delivery d JOIN private_contact_intents i ON i.id=d.contact_id WHERE d.accepted_at IS NULL AND d.next_attempt<=unixepoch() AND (d.lease_until IS NULL OR d.lease_until<unixepoch()) ORDER BY d.next_attempt,i.id LIMIT 1")
            .fetch_optional(&mut *tx).await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        let id = Uuid::parse_str(row.try_get("id")?)?;
        let lease = Uuid::new_v4();
        let envelope_json: String = row.try_get("envelope_json")?;
        let recipient: String = row.try_get("recipient")?;
        crate::contact::validate_envelope(&envelope_json, &recipient)
            .context("invalid retained private contact envelope")?;
        let relays: Vec<String> = serde_json::from_str(&row.try_get::<String, _>("relays_json")?)?;
        sqlx::query("UPDATE private_contact_delivery SET lease_id=?,lease_until=unixepoch()+90 WHERE contact_id=?")
            .bind(lease.to_string()).bind(id.to_string()).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(Some(ContactDelivery {
            contact_id: id,
            lease_id: lease,
            envelope_json,
            recipient,
            relays,
        }))
    }

    pub async fn finish_contact_delivery(
        &self,
        work: &ContactDelivery,
        accepted_relay: Option<&str>,
    ) -> Result<()> {
        if accepted_relay.is_some_and(|r| !work.relays.iter().any(|known| known == r)) {
            bail!("unselected contact acknowledgment relay");
        }
        let changed=sqlx::query("UPDATE private_contact_delivery SET lease_id=NULL,lease_until=NULL,attempts=MIN(attempts+1,30),next_attempt=unixepoch()+MIN(3600,30*(1<<MIN(attempts,7))),accepted_relay=?,accepted_at=CASE WHEN ? IS NULL THEN NULL ELSE unixepoch() END WHERE contact_id=? AND lease_id=? AND accepted_at IS NULL")
            .bind(accepted_relay).bind(accepted_relay).bind(work.contact_id.to_string()).bind(work.lease_id.to_string()).execute(&self.pool).await?;
        if changed.rows_affected() != 1 {
            bail!("private contact delivery lease changed");
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "contact_tests.rs"]
mod tests;
