//! Native read-only private-contact correlation for four-journal maintenance.
use anyhow::{Result, bail};
use futures_util::TryStreamExt as _;
use sqlx::{Row as _, SqlitePool};
use uuid::Uuid;

pub(crate) async fn verify(pool: &SqlitePool) -> Result<()> {
    // Split hash/source checks so indexed private identities are looked up
    // while scanning existing public rows once, rather than a large OR join.
    let invalid: bool = sqlx::query_scalar(r#"
      SELECT
        EXISTS(SELECT 1 FROM strike_receive_requests r
          LEFT JOIN private_contact_intents i ON i.receive_request_id=r.receive_request_id
          WHERE (r.address_user='contact' OR r.credit_pool='contact')
          AND (i.id IS NULL OR r.address_user<>'contact' OR r.credit_pool<>'contact'))
        OR EXISTS(SELECT 1 FROM private_contact_fees f
          LEFT JOIN private_contact_delivery d USING(contact_id) WHERE d.contact_id IS NULL)
        OR EXISTS(SELECT 1 FROM private_contact_fees f
          JOIN private_contact_intents i ON i.id=f.contact_id
          JOIN strike_receive_requests r ON r.receive_request_id=i.receive_request_id
          WHERE f.amount_msat<>100000 OR r.amount_msat<>100000 OR f.invoice_payment_hash<>r.payment_hash)
        OR EXISTS(SELECT 1 FROM private_contact_intents i JOIN zap_receive_context z USING(receive_request_id))
        OR EXISTS(SELECT 1 FROM settled_payments p
          JOIN strike_receive_requests r ON r.payment_hash=lower(p.payment_hash)
          JOIN private_contact_intents i ON i.receive_request_id=r.receive_request_id)
        OR EXISTS(SELECT 1 FROM private_contact_fees f
          JOIN settled_payments p ON p.source='strike' AND p.source_id=f.receive_id)
        OR EXISTS(SELECT 1 FROM private_contact_fees f
          JOIN ledger_entries e ON e.payment_source='strike' AND e.payment_source_id=f.receive_id)
        OR EXISTS(SELECT 1 FROM ledger_entries e
          JOIN private_contact_fees f ON f.invoice_payment_hash=lower(e.payment_hash))
    "#).fetch_one(pool).await?;
    if invalid {
        bail!("private fee purpose, delivery or public-accounting correlation failed");
    }
    let mut rows=sqlx::query("SELECT i.*,r.amount_msat,r.payment_hash,r.invoice,r.description_hash AS invoice_description,r.address_user,r.credit_pool,d.accepted_relay FROM private_contact_intents i LEFT JOIN strike_receive_requests r ON r.receive_request_id=i.receive_request_id LEFT JOIN private_contact_delivery d ON d.contact_id=i.id ORDER BY i.id").fetch(pool);
    let mut seen = 0usize;
    while let Some(row) = rows.try_next().await? {
        seen += 1;
        if seen > 10_000 {
            bail!("private contact history exceeds its admitted bound");
        }
        let id = Uuid::parse_str(row.try_get("id")?)?;
        if id.get_version_num() != 4 {
            bail!("invalid retained contact identity");
        }
        for field in [
            "capability_hash",
            "reply_email_hash",
            "envelope_id",
            "description_hash",
        ] {
            crate::nostr_event::validate_hex(&row.try_get::<String, _>(field)?, 32)?;
        }
        let recipient: String = row.try_get("recipient")?;
        let envelope = crate::contact::validate_envelope(
            &row.try_get::<String, _>("envelope_json")?,
            &recipient,
        )?;
        let description = crate::contact::description_hash(
            id,
            &envelope.event().id,
            &recipient,
            &row.try_get::<String, _>("reply_email_hash")?,
        )?;
        if envelope.event().id != row.try_get::<String, _>("envelope_id")?
            || description != row.try_get::<String, _>("description_hash")?
        {
            bail!("retained contact envelope or purpose commitment changed");
        }
        let raw: String = row.try_get("relays_json")?;
        if raw.len() > 16384 {
            bail!("retained inbox policy exceeds bound");
        }
        let relays: Vec<String> = serde_json::from_str(&raw)?;
        if relays.is_empty() || relays.len() > 8 {
            bail!("invalid retained inbox policy");
        }
        for relay in &relays {
            crate::public_network::validate_destination(relay, "wss")?;
        }
        if let Some(relay) = row.try_get::<Option<String>, _>("accepted_relay")? {
            if !relays.contains(&relay) {
                bail!("contact acknowledgment uses an unselected inbox relay");
            }
        }
        if let Some(claim) = row.try_get::<Option<String>, _>("issuance_claim")? {
            if Uuid::parse_str(&claim)?.get_version_num() != 4 {
                bail!("invalid retained contact issuance claim");
            }
        }
        if let Some(request) = row.try_get::<Option<String>, _>("receive_request_id")? {
            Uuid::parse_str(&request)?;
            if row.try_get::<i64, _>("amount_msat")? != i64::try_from(crate::contact::FEE_MSAT)?
                || row.try_get::<String, _>("invoice_description")? != description
                || row.try_get::<String, _>("address_user")? != "contact"
                || row.try_get::<String, _>("credit_pool")? != "contact"
            {
                bail!("retained contact invoice has a changed purpose or amount");
            }
            crate::bolt11::verify(
                &row.try_get::<String, _>("invoice")?,
                crate::contact::FEE_MSAT,
                &row.try_get::<String, _>("payment_hash")?,
                &description,
            )?;
        }
    }
    Ok(())
}
