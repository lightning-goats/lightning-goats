//! Cashu treasury issuance uses the existing receive-only BTC adapter. It cannot
//! create contribution credit and never retries an uncertain provider creation.
use super::*;
use crate::cashu::{receipt_authority::ReceiptPolicy, treasury::Policy};
use anyhow::ensure;
use lightning_goats_cashu_receipt::treasury::{Destination, Issued, hex_digest};
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

impl StrikeRuntime {
    /// Returns None for a retained uncertain creation. It never submits a fresh
    /// request merely because the original response or local checkpoint was lost.
    pub async fn create_cashu_treasury_invoice(
        &self,
        ledger: &LedgerStore,
        registration: &ReceiptPolicy,
        policy: &Policy,
        id: Uuid,
    ) -> Result<Option<Issued>> {
        let intent = ledger
            .cashu_treasury_intent(id)
            .await?
            .context("unknown treasury intent")?;
        ensure!(
            intent.core_epoch == registration.core_epoch
                && intent.wallet_instance == registration.wallet_instance
                && intent.wallet_epoch == registration.wallet_epoch,
            "treasury registration changed"
        );
        self.client.check_cashu_destination(&intent.destination)?;
        if let Some(issued) = ledger.cashu_treasury_invoice(id).await? {
            return Ok(Some(issued));
        }
        policy.check_new(&intent)?;
        let _permit = self
            .issuance_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| InvoiceCapacityError)?;
        let admission = ledger
            .reserve_invoice()
            .await?
            .ok_or(InvoiceCapacityError)?;
        let result = async {
            let Some(claim) = ledger.claim_cashu_treasury_issuance(id).await? else {
                return ledger.cashu_treasury_invoice(id).await;
            };
            let created = tokio::time::timeout(
                Duration::from_secs(20),
                self.client.create_bolt11_receive_request_inner(
                    intent.deposit_sats * 1000,
                    &hex_digest(intent.digest()?),
                    intent.invoice_expiry_seconds,
                    true,
                ),
            )
            .await
            .context("treasury creation outcome uncertain")??;
            ledger
                .record_cashu_treasury_invoice(
                    id,
                    claim,
                    &StoredStrikeReceiveRequest {
                        receive_request_id: created.receive_request_id,
                        address_user: "cashu_treasury".into(),
                        credit_pool: "cashu_treasury".into(),
                        amount_msat: intent.deposit_sats * 1000,
                        description_hash: created.description_hash,
                        payment_hash: created.payment_hash,
                        invoice: created.invoice,
                        created_provider: created.created_provider,
                    },
                )
                .await?;
            ledger.cashu_treasury_invoice(id).await
        }
        .await;
        if ledger.finish_invoice_admission(admission).await.is_err() {
            tracing::warn!("treasury invoice admission will expire durably");
        }
        result
    }

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
                    address_user: "cashu_treasury".into(),
                    credit_pool: "cashu_treasury".into(),
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
