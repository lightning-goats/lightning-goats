//! Settlement evidence returned by the project custody collector.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Immutable terms for a core execution claim. The payload digest binds the
/// collector's allocation list without copying private claim evidence into core.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRequest {
    pub reservation: crate::reward_funding::Reservation,
    pub member: String,
    pub disposition_id: uuid::Uuid,
    pub invoice: String,
    pub payment_hash: String,
    pub description_hash: String,
    pub gross_sats: u64,
    pub amount_sat: u64,
    pub maximum_fee_sat: u64,
    pub nip57: bool,
    pub collector_payload_digest: String,
}

impl ExecutionRequest {
    pub fn validate(&self) -> Result<u64> {
        self.reservation.validate()?;
        crate::nostr_event::validate_public_key(&self.member)?;
        for digest in [
            &self.payment_hash,
            &self.description_hash,
            &self.collector_payload_digest,
        ] {
            crate::nostr_event::validate_hex(digest, 32)?;
        }
        ensure!(
            self.member != self.reservation.owner
                && !self.disposition_id.is_nil()
                && self.invoice.len() <= 8192
                && self.amount_sat > 0
                && self.gross_sats <= self.reservation.reserved_sats
                && self
                    .amount_sat
                    .checked_add(self.maximum_fee_sat)
                    .is_some_and(|total| total <= self.gross_sats),
            "Cashu payout exceeds its reservation or has an invalid identity"
        );
        let msat = self
            .amount_sat
            .checked_mul(1000)
            .ok_or_else(|| anyhow::anyhow!("payout amount overflow"))?;
        let verifier = if self.nip57 {
            crate::bolt11::verify
        } else {
            crate::bolt11::verify_payout
        };
        let parsed = verifier(
            &self.invoice,
            msat,
            &self.payment_hash,
            &self.description_hash,
        )
        .map_err(|_| anyhow::anyhow!("Cashu payout differs from the signed invoice"))?;
        Ok(parsed
            .expires_at()
            .ok_or_else(|| anyhow::anyhow!("invalid Cashu payout expiry"))?
            .as_secs())
    }
}

#[derive(Deserialize)]
#[serde(tag = "state", rename_all = "UPPERCASE", deny_unknown_fields)]
pub enum CollectorResult {
    Unreserved,
    Unquoted,
    Unpaid {
        quote: String,
    },
    Pending {
        quote: String,
    },
    Paid {
        quote: String,
        amount_sat: u64,
        fee_sat: u64,
        payment_hash: String,
        payment_preimage: String,
    },
}

impl CollectorResult {
    /// Return the actual fee only when the collector proves payment of this invoice.
    pub fn verified_fee(
        &self,
        expected_hash: &str,
        amount: u64,
        fee_limit: u64,
    ) -> Result<Option<u64>> {
        let quote = match self {
            Self::Unreserved | Self::Unquoted => return Ok(None),
            Self::Unpaid { quote } | Self::Pending { quote } | Self::Paid { quote, .. } => quote,
        };
        ensure!(
            !quote.is_empty() && quote.len() <= 1024 && !quote.chars().any(char::is_control),
            "invalid collector quote"
        );
        let Self::Paid {
            amount_sat,
            fee_sat,
            payment_hash,
            payment_preimage,
            ..
        } = self
        else {
            return Ok(None);
        };
        let mut preimage = [0u8; 32];
        hex::decode_to_slice(payment_preimage, &mut preimage)?;
        ensure!(
            *amount_sat == amount
                && *fee_sat <= fee_limit
                && payment_hash == expected_hash
                && hex::encode(Sha256::digest(preimage)) == expected_hash,
            "collector receipt differs from authorized payout"
        );
        Ok(Some(*fee_sat))
    }
}

/// Obtain the routing reserve and exact input fees for the signed probe before choosing
/// the recipient amount. The collector keeps proofs out of this request.
#[cfg(unix)]
pub async fn payout_fee(
    socket: &std::path::Path,
    peer_uid: u32,
    invoice: &super::invoice::ValidatedInvoice,
    feed_id: uuid::Uuid,
    gross_sat: u64,
    allocations: &[super::funding::RetainedAllocation],
) -> Result<u64> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Quote {
        payment_hash: String,
        amount_sat: u64,
        routing_reserve_sat: u64,
        input_fee_sat: u64,
        expires_at: u64,
    }
    crate::private_ipc::socket_path(socket)?;
    let quote: Quote = crate::private_ipc::request(
        socket,
        peer_uid,
        &serde_json::json!({
            "quote_invoice": invoice.encoded(), "amount_sat": invoice.amount_sats(),
            "feed_id": feed_id, "gross_sat": gross_sat, "retained_allocations": allocations
        }),
        65536,
        30,
    )
    .await?;
    let now = u64::try_from(crate::cyberherd::calendar::now()?)?;
    ensure!(
        quote.payment_hash == invoice.payment_hash()
            && quote.amount_sat == invoice.amount_sats()
            && now < quote.expires_at.min(invoice.expires_at()),
        "fee observation differs from live payout invoice"
    );
    quote
        .routing_reserve_sat
        .checked_add(quote.input_fee_sat)
        .ok_or_else(|| anyhow::anyhow!("Cashu fee sum overflow"))
}

/// Read settlement from the collector's core-only socket. An absent receipt
/// leaves the reservation outstanding, including after invoice expiry.
#[cfg(unix)]
pub async fn settled_fee(
    collector: &crate::config::BreezCollectorConfig,
    claim: &ExecutionRequest,
) -> Result<Option<u64>> {
    claim.validate()?;
    let lookup = serde_json::json!({
        "payout_receipt": claim.disposition_id,
        "feed_id": claim.reservation.request_id,
        "invoice_digest": hex::encode(Sha256::digest(claim.invoice.as_bytes())),
        "payment_hash": claim.payment_hash,
    });
    let receipt: Option<CollectorResult> =
        crate::private_ipc::request(&collector.socket, collector.peer_uid, &lookup, 65536, 20)
            .await?;
    match receipt {
        None => Ok(None),
        Some(receipt @ CollectorResult::Paid { .. }) => {
            receipt.verified_fee(&claim.payment_hash, claim.amount_sat, claim.maximum_fee_sat)
        }
        Some(_) => anyhow::bail!("unexpected core payout receipt state"),
    }
}

/// Submit only the collector payload committed by the fresh core claim.
#[cfg(unix)]
pub async fn submit(
    socket: &std::path::Path,
    peer_uid: u32,
    lease: &crate::core_bridge::AuthenticatedCashuLease,
    payload: &serde_json::Value,
) -> Result<CollectorResult> {
    crate::private_ipc::socket_path(socket)?;
    let digest = hex::encode(Sha256::digest(serde_json::to_vec(payload)?));
    let terms = lease.terms(crate::cyberherd::calendar::now()?)?;
    ensure!(
        terms.collector_payload_digest == digest,
        "collector request differs from core execution claim"
    );
    let reply: CollectorResult =
        crate::private_ipc::request(socket, peer_uid, payload, 65536, 30).await?;
    reply.verified_fee(&terms.payment_hash, terms.amount_sat, terms.maximum_fee_sat)?;
    Ok(reply)
}

/// Query the saved disposition over the collector socket. This request contains
/// no grant allocations and cannot create a quote or submit proofs.
#[cfg(unix)]
pub async fn lookup(
    socket: &std::path::Path,
    peer_uid: u32,
    invoice: &super::invoice::ValidatedInvoice,
    fee_limit: u64,
) -> Result<CollectorResult> {
    let reply =
        lookup_disposition(socket, peer_uid, invoice.intent_id(), invoice.encoded()).await?;
    reply.verified_fee(invoice.payment_hash(), invoice.amount_sats(), fee_limit)?;
    Ok(reply)
}

#[cfg(unix)]
async fn lookup_disposition(
    socket: &std::path::Path,
    peer_uid: u32,
    id: uuid::Uuid,
    invoice: &str,
) -> Result<CollectorResult> {
    crate::private_ipc::socket_path(socket)?;
    crate::private_ipc::request(
        socket,
        peer_uid,
        &serde_json::json!({"reconcile_disposition":id,"invoice":invoice}),
        65536,
        30,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[cfg(unix)]
    #[tokio::test]
    async fn lookup_sends_only_the_original_identity_and_invoice() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("collector.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let uid = crate::private_ipc::uid().unwrap();
        let id = uuid::Uuid::new_v4();
        let server = tokio::spawn(async move {
            for state in [
                json!({"state":"UNQUOTED"}),
                json!({"state":"UNPAID","quote":"saved"}),
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request: serde_json::Value =
                    crate::private_ipc::read(&mut stream, 65536).await.unwrap();
                assert_eq!(
                    request,
                    json!({"reconcile_disposition":id,"invoice":"fixture"})
                );
                crate::private_ipc::write(&mut stream, &state, 65536)
                    .await
                    .unwrap();
            }
        });
        assert!(matches!(
            lookup_disposition(&socket, uid, id, "fixture")
                .await
                .unwrap(),
            CollectorResult::Unquoted
        ));
        assert!(matches!(
            lookup_disposition(&socket, uid, id, "fixture")
                .await
                .unwrap(),
            CollectorResult::Unpaid { .. }
        ));
        server.await.unwrap();
    }

    #[test]
    fn only_an_exact_paid_receipt_reports_a_fee() {
        let hash = hex::encode(Sha256::digest([7u8; 32]));
        let paid = json!({"state":"PAID","quote":"fixture","amount_sat":12,"fee_sat":1,
            "payment_hash":hash,"payment_preimage":hex::encode([7u8;32])});
        let result = serde_json::from_value::<CollectorResult>(paid.clone()).unwrap();
        assert_eq!(result.verified_fee(&hash, 12, 4).unwrap(), Some(1));
        assert!(result.verified_fee(&hash, 13, 4).is_err());
        assert!(result.verified_fee(&hash, 12, 0).is_err());
        let mut changed = paid;
        changed["payment_preimage"] = json!(hex::encode([8u8; 32]));
        assert!(
            serde_json::from_value::<CollectorResult>(changed)
                .unwrap()
                .verified_fee(&hash, 12, 4)
                .is_err()
        );
        let pending: CollectorResult =
            serde_json::from_value(json!({"state":"PENDING","quote":"fixture"})).unwrap();
        assert_eq!(pending.verified_fee(&hash, 12, 4).unwrap(), None);
    }
}
