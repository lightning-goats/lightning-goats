//! Generate pre-cutover receive evidence for recovery and rollback fixtures.
use super::*;
use crate::cashu::{receipt_authority::ReceiptPolicy, treasury::Policy};
impl StrikeRuntime {
    /// Returns None for a retained uncertain creation. It never submits a fresh
    /// request merely because the original response or local checkpoint was lost.
    pub(crate) async fn create_cashu_treasury_invoice(
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
        ensure!(
            self.client
                .base_url
                .host_str()
                .and_then(|h| h.parse::<std::net::IpAddr>().ok())
                .is_some_and(|ip| ip.is_loopback()),
            "historical fixture requires loopback"
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
                        address_user: TREASURY_USER.into(),
                        credit_pool: TREASURY_USER.into(),
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
}
