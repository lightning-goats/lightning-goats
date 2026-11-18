//! Validation of a core-issued treasury invoice before any mint quote request.
//! Caller must authenticate the core peer before decoding. This check grants no
//! execution authority: durable proof reservation and quote journaling follow it.
use anyhow::{Result, ensure};
use cdk::lightning_invoice::{Bolt11Invoice, Currency};
use lightning_goats_cashu_receipt::treasury::{Destination, Issued};
use std::time::Duration;
use uuid::Uuid;

/// Protected wallet policy, independent of fields received over the socket.
#[derive(Clone)]
pub struct TreasuryBinding {
    pub core_epoch: Uuid,
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    pub destination: Destination,
    pub mint_id: String,
    pub policy_version: String,
}

impl TreasuryBinding {
    /// `now` is the local trusted clock. Expiry forbids new quote/submission work;
    /// it must never be used to infer nonpayment for an existing operation.
    pub(crate) fn validate_scope(&self, issued: &Issued) -> Result<()> {
        issued.intent.validate()?;
        self.destination.validate()?;
        let intent = &issued.intent;
        ensure!(
            intent.core_epoch == self.core_epoch
                && intent.wallet_instance == self.wallet_instance
                && intent.wallet_epoch == self.wallet_epoch
                && intent.destination == self.destination
                && intent.mint_id == self.mint_id
                && intent.treasury_policy == self.policy_version,
            "treasury instruction is outside protected wallet scope"
        );
        Ok(())
    }

    pub fn validate_new_invoice(&self, issued: &Issued, now: u64) -> Result<()> {
        self.validate_scope(issued)?;
        let intent = &issued.intent;
        ensure!(
            !issued.receive_request_id.is_nil() && issued.invoice.len() <= 16_384,
            "missing or oversized bound treasury invoice"
        );
        lightning_goats_nostr_event::validate_hex(&issued.payment_hash, 32)?;
        let invoice: Bolt11Invoice = issued.invoice.parse()?;
        invoice.check_signature()?;
        ensure!(
            invoice.currency() == Currency::Bitcoin
                && invoice.amount_milli_satoshis() == intent.deposit_sats.checked_mul(1000)
                && invoice.payment_hash().to_string() == issued.payment_hash,
            "treasury invoice network, amount or payment hash mismatch"
        );
        let created = invoice.duration_since_epoch();
        let expires = created
            .checked_add(invoice.expiry_time())
            .ok_or_else(|| anyhow::anyhow!("treasury invoice expiry overflow"))?;
        ensure!(
            created <= Duration::from_secs(now)
                && Duration::from_secs(now) < expires
                && invoice.expiry_time().as_secs() <= intent.invoice_expiry_seconds,
            "treasury invoice is future, expired or exceeds its frozen lifetime"
        );
        Ok(())
    }
}

/// Journal claim taken before CDK quote/proof reservation.
pub(crate) struct ClaimedPreparation {
    pub operation: Option<Uuid>,
    pub quote: cdk::wallet::MeltQuote,
    pub ys: Vec<cdk::nuts::PublicKey>,
}

/// Private restart context for a bound, never-submitted CDK preparation.
pub(crate) struct PreparedContext {
    pub operation: Uuid,
    pub quote: cdk::wallet::MeltQuote,
    pub input_fee: u64,
    pub ys: Vec<cdk::nuts::PublicKey>,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cdk::lightning_invoice::{InvoiceBuilder, PaymentSecret};
    use cdk::secp256k1::{Secp256k1, SecretKey};
    use lightning_goats_cashu_receipt::treasury::{Intent, PURPOSE, Source};

    fn signed_invoice(currency: Currency, msat: u64, created: u64, expiry: u64) -> String {
        signed_invoice_hash(currency, msat, created, expiry, "ab")
    }

    pub(crate) fn signed_invoice_hash(
        currency: Currency,
        msat: u64,
        created: u64,
        expiry: u64,
        hash: &str,
    ) -> String {
        signed_invoice_payment_hash(currency, msat, created, expiry, &hash.repeat(32))
    }

    pub(crate) fn signed_invoice_payment_hash(
        currency: Currency,
        msat: u64,
        created: u64,
        expiry: u64,
        hash: &str,
    ) -> String {
        let key = SecretKey::from_slice(&[42; 32]).unwrap();
        let secp = Secp256k1::new();
        InvoiceBuilder::new(currency)
            .description("Internal treasury fixture".into())
            .payment_hash(hash.parse().unwrap())
            .payment_secret(PaymentSecret([44; 32]))
            .duration_since_epoch(Duration::from_secs(created))
            .min_final_cltv_expiry_delta(18)
            .expiry_time(Duration::from_secs(expiry))
            .amount_milli_satoshis(msat)
            .build_signed(|message| secp.sign_ecdsa_recoverable(message, &key))
            .unwrap()
            .to_string()
    }

    pub(crate) fn fixture() -> (TreasuryBinding, Issued) {
        let binding = TreasuryBinding {
            core_epoch: Uuid::from_u128(1),
            wallet_instance: Uuid::from_u128(2),
            wallet_epoch: Uuid::from_u128(3),
            destination: Destination {
                origin: "https://api.example.com/".into(),
                account_id: Uuid::from_u128(4),
                credential_fingerprint: "aa".repeat(32),
                network: "bitcoin".into(),
            },
            mint_id: "mint-one".into(),
            policy_version: "treasury-v1".into(),
        };
        let issued = Issued {
            intent: Intent {
                version: 1,
                purpose: PURPOSE.into(),
                id: Uuid::from_u128(5),
                core_epoch: binding.core_epoch,
                wallet_instance: binding.wallet_instance,
                wallet_epoch: binding.wallet_epoch,
                destination: binding.destination.clone(),
                mint_id: binding.mint_id.clone(),
                treasury_policy: binding.policy_version.clone(),
                input_sats: 101,
                deposit_sats: 100,
                maximum_fee_sats: 1,
                invoice_expiry_seconds: 300,
                sources: vec![Source {
                    ledger_id: 1,
                    acquisition_id: Uuid::from_u128(6),
                    receipt_digest: "cc".repeat(32),
                    input_sats: 101,
                }],
            },
            receive_request_id: Uuid::from_u128(7),
            payment_hash: "ab".repeat(32),
            invoice: signed_invoice(Currency::Bitcoin, 100_000, 1000, 300),
        };
        (binding, issued)
    }

    #[test]
    fn durable_invoice_binding_survives_expiry_and_refuses_replacement() {
        let (binding, issued) = fixture();
        let directory = crate::test_directory().unwrap();
        let path = directory.path().join("journal.sqlite");
        let mut journal = crate::journal::Journal::open(&path, binding.wallet_epoch, 128).unwrap();
        assert!(
            journal
                .bind_treasury_instruction(&binding, &issued, 1000)
                .unwrap()
        );
        drop(journal);
        let mut journal = crate::journal::Journal::open(&path, binding.wallet_epoch, 128).unwrap();
        assert!(
            !journal
                .bind_treasury_instruction(&binding, &issued, 2000)
                .unwrap()
        );
        let mut changed = issued.clone();
        changed.invoice = signed_invoice(Currency::Bitcoin, 100_000, 1100, 300);
        assert!(
            journal
                .bind_treasury_instruction(&binding, &changed, 1100)
                .is_err()
        );
        changed = issued.clone();
        changed.intent.id = Uuid::from_u128(98);
        // A fresh intent cannot adopt the original invoice/hash under another ID.
        assert!(
            journal
                .bind_treasury_instruction(&binding, &changed, 1000)
                .is_err()
        );
        assert!(
            !journal
                .bind_treasury_instruction(&binding, &issued, 2000)
                .unwrap()
        );
        let other_path = directory.path().join("other.sqlite");
        let mut other =
            crate::journal::Journal::open(&other_path, Uuid::from_u128(99), 128).unwrap();
        assert!(
            other
                .bind_treasury_instruction(&binding, &issued, 1000)
                .is_err()
        );
        // Expired first admission is refused even though replay remains readable.
        let fresh_path = directory.path().join("fresh.sqlite");
        let mut fresh =
            crate::journal::Journal::open(&fresh_path, binding.wallet_epoch, 128).unwrap();
        assert!(
            fresh
                .bind_treasury_instruction(&binding, &issued, 1300)
                .is_err()
        );
    }

    #[test]
    fn signed_invoices_must_match_scope_amount_hash_and_exact_time_boundary() {
        let (binding, issued) = fixture();
        binding.validate_new_invoice(&issued, 1000).unwrap();
        binding.validate_new_invoice(&issued, 1299).unwrap();
        assert!(binding.validate_new_invoice(&issued, 999).is_err());
        assert!(binding.validate_new_invoice(&issued, 1300).is_err());
        for (currency, amount, expiry) in [
            (Currency::BitcoinTestnet, 100_000, 300),
            (Currency::Bitcoin, 99_999, 300),
            (Currency::Bitcoin, 100_001, 300),
            (Currency::Bitcoin, 100_000, 301),
        ] {
            let mut bad = issued.clone();
            bad.invoice = signed_invoice(currency, amount, 1000, expiry);
            assert!(binding.validate_new_invoice(&bad, 1000).is_err());
        }
        for field in 0..9 {
            let mut bad = issued.clone();
            match field {
                0 => bad.intent.core_epoch = Uuid::from_u128(99),
                1 => bad.intent.wallet_epoch = Uuid::from_u128(99),
                2 => bad.intent.wallet_instance = Uuid::from_u128(99),
                3 => bad.intent.destination.account_id = Uuid::from_u128(99),
                4 => bad.intent.destination.credential_fingerprint = "dd".repeat(32),
                5 => bad.intent.mint_id = "other-mint".into(),
                6 => bad.intent.treasury_policy = "other-policy".into(),
                7 => bad.payment_hash = "ee".repeat(32),
                _ => bad.receive_request_id = Uuid::nil(),
            }
            assert!(binding.validate_new_invoice(&bad, 1000).is_err());
        }
    }
}
