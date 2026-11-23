//! Receipt-signing fixture only: no provider, ledger, settlement or publication.
use anyhow::{Result, anyhow};
use lightning_goats::{ledger::ZapReceiptJob, nostr_event::VerifiedEvent, zap::ZapRequest};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[path = "invoices.rs"]
#[allow(dead_code)]
mod invoices;

pub fn synthetic_job(receipt_signer: &str) -> Result<ZapReceiptJob> {
    let raw = include_str!("../fixtures/nip57-nak-request.json");
    let event = VerifiedEvent::from_json(raw)?;
    let payee = event
        .singleton_tag("p", true)?
        .ok_or_else(|| anyhow!("synthetic request has no payee"))?;
    let request = ZapRequest::validate(raw, payee, receipt_signer, 10_000)?;
    let created = SystemTime::now().duration_since(UNIX_EPOCH)?;
    let payment_hash: [u8; 32] = Sha256::digest(Uuid::new_v4().as_bytes()).into();
    let invoice = invoices::invoice_with_payment_hash(
        request.amount_msat(),
        &request.description_hash(),
        created,
        payment_hash,
    );
    Ok(ZapReceiptJob {
        settlement_id: -1,
        lease_id: Uuid::new_v4(),
        source_event_seq: 0,
        paid_at: created.as_secs(),
        invoice,
        request,
        signed_json: None,
        relay: "wss://relay.primal.net".into(),
    })
}
