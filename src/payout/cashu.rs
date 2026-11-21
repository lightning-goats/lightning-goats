//! Settlement evidence returned by the project custody collector.
use anyhow::{Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(tag = "state", rename_all = "UPPERCASE", deny_unknown_fields)]
pub enum CollectorResult {
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
            Self::Unquoted => return Ok(None),
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
