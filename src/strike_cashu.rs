//! Cashu treasury issuance uses the existing receive-only BTC adapter. It cannot
//! create contribution credit and never retries an uncertain provider creation.
use super::*;
use crate::cashu::treasury::TREASURY_USER;
use anyhow::ensure;
use lightning_goats_cashu_receipt::treasury::{Destination, Intent, hex_digest};
use sha2::Digest;

impl StrikeClient {
    pub(crate) fn check_cashu_destination(&self, destination: &Destination) -> Result<()> {
        destination.validate()?;
        ensure!(
            self.base_url.as_str() == destination.origin
                && hex::encode(Sha256::digest(self.api_key.as_bytes()))
                    == destination.credential_fingerprint,
            "treasury provider origin/credential changed"
        );
        Ok(())
    }

    /// Verify one original treasury receipt using the frozen invoice and intent.
    /// This is a provider read and has no access to the ledger writer.
    pub(crate) async fn verify_cashu_treasury_receive(
        &self,
        intent: &Intent,
        stored: &StoredStrikeReceiveRequest,
        receive_id: Uuid,
    ) -> Result<ReconciledStrikeReceive> {
        self.check_cashu_destination(&intent.destination)?;
        ensure!(
            stored.address_user == TREASURY_USER
                && stored.credit_pool == TREASURY_USER
                && stored.description_hash == hex_digest(intent.digest()?)
                && Some(stored.amount_msat) == intent.deposit_sats.checked_mul(1000),
            "stored treasury receive differs from its original purpose"
        );
        self.reconcile_completed_receive_inner(stored, receive_id, true)
            .await
    }
}

impl ReconciledStrikeReceive {
    pub(crate) fn treasury_evidence(&self) -> Result<serde_json::Value> {
        ensure!(
            self.receive_type == "LIGHTNING"
                && self.received.currency == "BTC"
                && self.conversion_rate.is_none(),
            "treasury requires an exact native BTC Lightning receipt"
        );
        Ok(
            json!({"receive_request_id":self.receive_request_id,"receive_id":self.receive_id,
            "payment_hash":self.payment_hash,"amount_msat":self.amount_msat,
            "receive_type":self.receive_type,"received":self.received,"credited":self.credited,
            "completed_provider":self.completed_provider}),
        )
    }
}

#[cfg(test)]
use lightning_goats_cashu_receipt::treasury::Issued;

#[cfg(test)]
impl StrikeRuntime {
    /// Read/adopt a known original provider resource after response/checkpoint
    /// loss. No invoice creation, replacement or wallet execution occurs here.
    pub async fn adopt_cashu_treasury_invoice(
        &self,
        ledger: &LedgerStore,
        id: Uuid,
        request_id: Uuid,
    ) -> Result<Issued> {
        let intent = ledger
            .cashu_treasury_intent(id)
            .await?
            .context("unknown treasury intent")?;
        self.client.check_cashu_destination(&intent.destination)?;
        let response = self.client.find_receive_request(request_id).await?;
        ensure!(
            response.receive_request_id == request_id
                && response.target_currency.as_deref() == Some("BTC"),
            "treasury provider request has wrong identity/target"
        );
        let description = hex_digest(intent.digest()?);
        validate_receive_request_response(&response, intent.deposit_sats * 1000, &description)?;
        let bolt11 = response
            .bolt11
            .context("treasury request omitted invoice")?;
        let claim = ledger
            .cashu_treasury_issuance_claim(id)
            .await?
            .context("treasury creation was not claimed")?;
        ledger
            .record_cashu_treasury_invoice(
                id,
                claim,
                &StoredStrikeReceiveRequest {
                    receive_request_id: request_id,
                    address_user: TREASURY_USER.into(),
                    credit_pool: TREASURY_USER.into(),
                    amount_msat: intent.deposit_sats * 1000,
                    description_hash: description,
                    payment_hash: bolt11.payment_hash.to_ascii_lowercase(),
                    invoice: bolt11.invoice,
                    created_provider: response.created,
                },
            )
            .await?;
        ledger
            .cashu_treasury_invoice(id)
            .await?
            .context("treasury adoption not bound")
    }
}

#[cfg(test)]
#[path = "historical_treasury_fixture.rs"]
mod historical_fixture;
