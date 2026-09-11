//! Verify the signed invoice, independently of provider JSON wrapper fields.
use anyhow::{Context, Result, bail};
use lightning_invoice::{Bolt11Invoice, Bolt11InvoiceDescriptionRef, Currency};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) fn verify(
    encoded: &str,
    amount_msat: u64,
    payment_hash: &str,
    description_hash: &str,
) -> Result<Bolt11Invoice> {
    let invoice: Bolt11Invoice = encoded.parse().context("invalid signed BOLT11 invoice")?;
    invoice
        .check_signature()
        .context("invalid BOLT11 signature")?;
    if invoice.currency() != Currency::Bitcoin {
        bail!("BOLT11 network must be Bitcoin mainnet");
    }
    if invoice.amount_milli_satoshis() != Some(amount_msat) || amount_msat == 0 {
        bail!("BOLT11 amount differs from requested millisatoshis");
    }
    if !invoice
        .payment_hash()
        .to_string()
        .eq_ignore_ascii_case(payment_hash)
    {
        bail!("BOLT11 payment hash differs from provider contract");
    }
    match invoice.description() {
        Bolt11InvoiceDescriptionRef::Hash(hash)
            if hash.0.to_string().eq_ignore_ascii_case(description_hash) => {}
        _ => bail!("BOLT11 must commit to the exact LNURL metadata hash"),
    }
    Ok(invoice)
}

pub(crate) fn verify_issuance_expiry(invoice: &Bolt11Invoice, expected_seconds: u64) -> Result<()> {
    verify_expiry_at(
        invoice,
        expected_seconds,
        SystemTime::now().duration_since(UNIX_EPOCH)?,
    )
}

fn verify_expiry_at(invoice: &Bolt11Invoice, expected_seconds: u64, now: Duration) -> Result<()> {
    if expected_seconds == 0 || invoice.expiry_time() != Duration::from_secs(expected_seconds) {
        bail!("BOLT11 expiry differs from requested expiry");
    }
    // Permit small provider clock skew, never an arbitrarily future invoice.
    if invoice.duration_since_epoch() > now.saturating_add(Duration::from_secs(30)) {
        bail!("BOLT11 timestamp is too far in the future");
    }
    if invoice.expires_at().is_none_or(|expires| expires <= now) {
        bail!("BOLT11 is already expired");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::secp256k1::{Secp256k1, SecretKey};
    use lightning_invoice::{RawBolt11Invoice, SignedRawBolt11Invoice};

    fn sign(raw: RawBolt11Invoice, key: u8) -> String {
        raw.sign(|hash| {
            Ok::<_, std::convert::Infallible>(
                Secp256k1::new()
                    .sign_ecdsa_recoverable(hash, &SecretKey::from_slice(&[key; 32]).unwrap()),
            )
        })
        .unwrap()
        .to_string()
    }

    #[test]
    fn rejects_bad_checksum_signature_network_amount_and_hashes() {
        let encoded = crate::test_invoices::invoice(2_340_000, &"11".repeat(32));
        let verified = verify(&encoded, 2_340_000, &"22".repeat(32), &"11".repeat(32)).unwrap();
        let (raw, _, _) = verified.into_signed_raw().into_parts();
        let invalid_signature = sign(raw.clone(), 43);
        let parsed: SignedRawBolt11Invoice = invalid_signature.parse().unwrap();
        assert!(!parsed.check_signature()); // A valid checksum does not establish a valid signature.
        assert!(
            verify(
                &invalid_signature,
                2_340_000,
                &"22".repeat(32),
                &"11".repeat(32)
            )
            .is_err()
        );
        let mut wrong_network = raw.clone();
        wrong_network.hrp.currency = Currency::BitcoinTestnet;
        assert!(
            verify(
                &sign(wrong_network, 42),
                2_340_000,
                &"22".repeat(32),
                &"11".repeat(32)
            )
            .is_err()
        );
        let mut no_amount = raw;
        no_amount.hrp.raw_amount = None;
        no_amount.hrp.si_prefix = None;
        assert!(
            verify(
                &sign(no_amount, 42),
                2_340_000,
                &"22".repeat(32),
                &"11".repeat(32)
            )
            .is_err()
        );
        assert!(verify(&encoded, 2_341_000, &"22".repeat(32), &"11".repeat(32)).is_err());
        assert!(verify(&encoded, 2_340_000, &"33".repeat(32), &"11".repeat(32)).is_err());
        assert!(verify(&encoded, 2_340_000, &"22".repeat(32), &"33".repeat(32)).is_err());
        let mut bad_checksum = encoded.into_bytes();
        let last = bad_checksum.last_mut().unwrap();
        *last = if *last == b'q' { b'p' } else { b'q' };
        assert!(
            verify(
                std::str::from_utf8(&bad_checksum).unwrap(),
                2_340_000,
                &"22".repeat(32),
                &"11".repeat(32)
            )
            .is_err()
        );
    }

    #[test]
    fn issuance_checks_expiry_and_clock_but_late_settlement_can_verify_expired_invoice() {
        let encoded = crate::test_invoices::invoice(1_000, &"11".repeat(32));
        let invoice = verify(&encoded, 1_000, &"22".repeat(32), &"11".repeat(32)).unwrap();
        let created = invoice.duration_since_epoch();
        verify_expiry_at(&invoice, 300, created).unwrap();
        assert!(verify_expiry_at(&invoice, 301, created).is_err());
        assert!(verify_expiry_at(&invoice, 300, created - Duration::from_secs(31)).is_err());
        assert!(verify_expiry_at(&invoice, 300, created + Duration::from_secs(300)).is_err());
        // Recovery uses verify(), not issuance-only expiry checks.
        verify(&encoded, 1_000, &"22".repeat(32), &"11".repeat(32)).unwrap();
    }
}
