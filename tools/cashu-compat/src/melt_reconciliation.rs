//! Mint observations are private evidence, not authoritative Strike settlement.
use anyhow::{Result, ensure};
use cdk::{
    mint_url::MintUrl,
    nuts::{CurrencyUnit, MeltQuoteBolt11Response, MeltQuoteState, PaymentMethod},
    wallet::MeltQuote,
};
use lightning_goats_cashu_receipt::treasury::Issued;
use sha2::{Digest, Sha256};

/// Original private recovery context loaded from the wallet journal. No proof
/// secrets are exposed here, and loading it never grants execution permission.
pub struct RecoveryContext {
    pub issued: Issued,
    pub quote: MeltQuote,
    pub derivation: crate::melt_derivation::MeltDerivation,
}
impl RecoveryContext {
    /// One bounded read of the original quote. None means unresolved, never
    /// permission to release proofs, retry payment or create another invoice.
    pub async fn poll_paid<T: cdk_http_client::Transport>(
        &self,
        transport: &T,
    ) -> Result<Option<MeltQuoteBolt11Response<String>>> {
        ensure!(
            self.quote.id == self.derivation.quote_id
                && self.quote.mint_url.as_ref() == Some(&self.derivation.mint_url)
                && self.quote.request == self.issued.invoice,
            "recovery lookup scope mismatch"
        );
        let observed = poll_original_quote(
            transport,
            &self.issued,
            &self.quote,
            &self.derivation.mint_url,
        )
        .await?;
        Ok((observed.state == MeltQuoteState::Paid).then_some(observed))
    }

    /// NUT-09 retrieves signatures for the original outputs; it never spends.
    /// Preserve the returned raw response as evidence before terminal accounting.
    pub async fn restore_missing_change<T: cdk_http_client::Transport>(
        &self,
        transport: &T,
        paid: &MeltQuoteBolt11Response<String>,
    ) -> Result<cdk::nuts::RestoreResponse> {
        validate_paid_quote(&self.issued, &self.quote, paid)?;
        ensure!(
            self.quote.id == self.derivation.quote_id
                && self.quote.mint_url.as_ref() == Some(&self.derivation.mint_url),
            "restore scope mismatch"
        );
        ensure!(
            paid.change.is_none(),
            "paid response already contains change evidence"
        );
        ensure!(
            self.derivation.outputs.len() <= 1024,
            "restore output bound exceeded"
        );
        if self.derivation.outputs.is_empty() {
            ensure!(
                self.derivation.maximum_change_sats == 0,
                "missing original change outputs"
            );
            return Ok(cdk::nuts::RestoreResponse {
                outputs: vec![],
                signatures: vec![],
            });
        }
        let response: cdk::nuts::RestoreResponse = transport
            .http_post(
                self.derivation.mint_url.join("v1/restore")?,
                None,
                &cdk::nuts::RestoreRequest {
                    outputs: self.derivation.outputs.clone(),
                },
            )
            .await
            .map_err(|_| anyhow::anyhow!("original change restoration failed"))?;
        normalize_restored_change(&self.derivation, &response)?;
        Ok(response)
    }

    /// Build the original read-only quote lookup; never create a replacement.
    pub fn quote_status_url(&self) -> Result<url::Url> {
        quote_status_url(&self.derivation.mint_url, &self.quote.id)
    }
}

fn quote_status_url(mint: &MintUrl, id: &str) -> Result<url::Url> {
    ensure!(
        !id.is_empty()
            && id.len() <= 256
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "quote identity is not supported by the constrained transport"
    );
    let mut url = mint.join("v1/melt/quote/bolt11/")?;
    url.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("invalid quote origin"))?
        .pop_if_empty()
        .push(id);
    Ok(url)
}

pub(crate) async fn poll_original_quote<T: cdk_http_client::Transport>(
    transport: &T,
    issued: &Issued,
    original: &MeltQuote,
    mint: &MintUrl,
) -> Result<MeltQuoteBolt11Response<String>> {
    ensure!(
        original.mint_url.as_ref() == Some(mint),
        "original mint origin differs"
    );
    validate_original_quote(issued, original)?;
    let observed: MeltQuoteBolt11Response<String> = transport
        .http_get(quote_status_url(mint, &original.id)?, None)
        .await
        .map_err(|_| anyhow::anyhow!("original melt quote lookup failed"))?;
    validate_observed_terms(issued, original, &observed)?;
    if observed.state == MeltQuoteState::Paid {
        validate_paid_preimage(issued, &observed)?;
    } else {
        ensure!(
            observed.payment_preimage.is_none()
                && observed.change.as_ref().is_none_or(Vec::is_empty),
            "nonpaid quote carries conflicting paid evidence"
        );
    }
    Ok(observed)
}

fn validate_original_quote(issued: &Issued, original: &MeltQuote) -> Result<()> {
    issued.intent.validate()?;
    let invoice: cdk::lightning_invoice::Bolt11Invoice = issued.invoice.parse()?;
    invoice.check_signature()?;
    ensure!(
        invoice.currency() == cdk::lightning_invoice::Currency::Bitcoin
            && invoice.amount_milli_satoshis() == issued.intent.deposit_sats.checked_mul(1000)
            && invoice.payment_hash().to_string() == issued.payment_hash,
        "retained treasury invoice mismatch"
    );
    ensure!(
        original.request == issued.invoice
            && original.unit == CurrencyUnit::Sat
            && original.payment_method == PaymentMethod::BOLT11
            && u64::from(original.amount) == issued.intent.deposit_sats
            && u64::from(original.fee_reserve) <= issued.intent.maximum_fee_sats,
        "retained quote differs from treasury instruction"
    );
    Ok(())
}

/// Validate the original quote's paid observation, including its invoice preimage.
/// Expiry is intentionally not checked: a late observation must reconcile the
/// original operation. Change ownership and independent Strike deposit evidence
/// are additional requirements before any settlement report can be exported.
pub fn validate_paid_quote(
    issued: &Issued,
    original: &MeltQuote,
    observed: &MeltQuoteBolt11Response<String>,
) -> Result<()> {
    validate_original_quote(issued, original)?;
    validate_observed_terms(issued, original, observed)?;
    validate_paid_preimage(issued, observed)
}

fn validate_paid_preimage(
    issued: &Issued,
    observed: &MeltQuoteBolt11Response<String>,
) -> Result<()> {
    ensure!(observed.state == MeltQuoteState::Paid, "quote is not paid");
    let preimage = observed
        .payment_preimage
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("paid observation lacks preimage"))?;
    lightning_goats_nostr_event::validate_hex(preimage, 32)?;
    let mut bytes = [0u8; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&preimage[index * 2..index * 2 + 2], 16)?;
    }
    ensure!(
        lightning_goats_cashu_receipt::treasury::hex_digest(Sha256::digest(bytes).into())
            == issued.payment_hash,
        "paid preimage does not match original invoice"
    );
    Ok(())
}

fn validate_observed_terms(
    issued: &Issued,
    original: &MeltQuote,
    observed: &MeltQuoteBolt11Response<String>,
) -> Result<()> {
    ensure!(
        observed.quote == original.id
            && observed.amount == original.amount
            && observed.fee_reserve == original.fee_reserve
            && observed.expiry == original.expiry
            && observed.method == PaymentMethod::BOLT11
            && observed
                .request
                .as_ref()
                .is_none_or(|request| request == &issued.invoice)
            && observed
                .unit
                .as_ref()
                .is_none_or(|unit| *unit == CurrencyUnit::Sat),
        "observation differs from original BOLT11 quote"
    );
    ensure!(
        observed
            .change
            .as_ref()
            .is_none_or(|change| change.len() <= 1024),
        "quote change exceeds bound"
    );
    Ok(())
}

/// Normalize NUT-09 response order without accepting unrelated, duplicated or
/// sparse outputs as a complete NUT-08 change result. Cryptographic ownership
/// verification remains required; matching output IDs alone is insufficient.
pub fn normalize_restored_change(
    derivation: &crate::melt_derivation::MeltDerivation,
    response: &cdk::nuts::RestoreResponse,
) -> Result<Vec<cdk::nuts::BlindSignature>> {
    use std::collections::BTreeMap;
    ensure!(
        derivation.outputs.len() <= 1024
            && response.outputs.len() <= derivation.outputs.len()
            && response.outputs.len() == response.signatures.len(),
        "restore response count mismatch"
    );
    let mut by_output = BTreeMap::new();
    for (output, signature) in response.outputs.iter().zip(&response.signatures) {
        ensure!(
            signature.amount > cdk::Amount::ZERO
                && signature.keyset_id == output.keyset_id
                && (output.amount == cdk::Amount::ZERO || output.amount == signature.amount),
            "restore signature terms mismatch"
        );
        ensure!(
            by_output
                .insert(output.blinded_secret.to_bytes(), (output, signature))
                .is_none(),
            "duplicate restored output"
        );
    }
    let mut signatures = Vec::new();
    let mut missing = false;
    for output in &derivation.outputs {
        if let Some((echo, signature)) = by_output.remove(&output.blinded_secret.to_bytes()) {
            ensure!(
                !missing && echo.keyset_id == output.keyset_id,
                "restored change is sparse or changes keyset"
            );
            signatures.push(signature.clone());
        } else {
            missing = true;
        }
    }
    ensure!(by_output.is_empty(), "restore includes unrelated output");
    Ok(signatures)
}

/// Reconstruct candidate change proofs from retained outputs and signatures.
/// The returned local state is not fresh mint evidence: callers must obtain
/// exact NUT-07 states and call verify_owned_change before committing accounting.
pub fn reconstruct_change(
    derivation: &crate::melt_derivation::MeltDerivation,
    seed: &[u8; 64],
    mint_id: &str,
    keys: &crate::intake::PinnedKeys,
    signatures: &[cdk::nuts::BlindSignature],
) -> Result<Vec<cdk::wallet::types::ProofInfo>> {
    use cdk::nuts::{PreMintSecrets, Proof, ProofDleq, State};
    ensure!(
        derivation.outputs.len() <= 1024
            && signatures.len() <= derivation.outputs.len()
            && derivation.counter_end.checked_sub(derivation.counter_start)
                == Some(u32::try_from(derivation.outputs.len())?),
        "invalid change reconstruction bounds"
    );
    if derivation.outputs.is_empty() {
        ensure!(
            derivation.maximum_change_sats == 0,
            "missing change derivation"
        );
        return Ok(vec![]);
    }
    let keyset = derivation.outputs[0].keyset_id;
    let derived = PreMintSecrets::from_seed_blank(
        keyset,
        derivation.counter_start,
        seed,
        derivation.maximum_change_sats.into(),
    )?;
    ensure!(
        derived.blinded_messages() == derivation.outputs,
        "change reconstruction seed mismatch"
    );
    let mut total = 0u64;
    let mut proofs = Vec::new();
    for (signature, item) in signatures.iter().zip(&derived.secrets) {
        ensure!(
            signature.keyset_id == keyset && signature.amount > cdk::Amount::ZERO,
            "change signature terms mismatch"
        );
        let key = keys.key(mint_id, keyset, u64::from(signature.amount))?;
        let dleq = signature
            .dleq
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("restored signature lacks DLEQ"))?;
        let mut proof = Proof::new(
            signature.amount,
            keyset,
            item.secret.clone(),
            cdk::dhke::unblind_message(&signature.c, &item.r, &key)?,
        );
        proof.dleq = Some(ProofDleq::new(
            dleq.e.clone(),
            dleq.s.clone(),
            item.r.clone(),
        ));
        proof
            .verify_dleq(key)
            .map_err(|_| anyhow::anyhow!("restored signature has invalid DLEQ"))?;
        total = total
            .checked_add(u64::from(signature.amount))
            .ok_or_else(|| anyhow::anyhow!("restored change overflow"))?;
        proofs.push(cdk::wallet::types::ProofInfo::new(
            proof,
            derivation.mint_url.clone(),
            State::Unspent,
            CurrencyUnit::Sat,
        )?);
    }
    ensure!(
        total <= derivation.maximum_change_sats,
        "restored change exceeds input value"
    );
    Ok(proofs)
}

/// Verify recovered change against the original blank outputs and wallet seed.
/// `signatures` must be the complete ordered change response for the original
/// operation. An absent/ambiguous response is not an empty change result.
#[allow(clippy::too_many_arguments)]
pub fn verify_owned_change(
    derivation: &crate::melt_derivation::MeltDerivation,
    seed: &[u8; 64],
    mint_id: &str,
    keys: &crate::intake::PinnedKeys,
    signatures: &[cdk::nuts::BlindSignature],
    owned: &[cdk::wallet::types::ProofInfo],
    fresh_states: &cdk::nuts::CheckStateResponse,
) -> Result<u64> {
    use cdk::nuts::{PreMintSecrets, State};
    use std::collections::{BTreeMap, BTreeSet};
    ensure!(
        derivation.outputs.len() <= 1024
            && signatures.len() <= derivation.outputs.len()
            && owned.len() == signatures.len()
            && fresh_states.states.len() == signatures.len(),
        "change proof count mismatch"
    );
    ensure!(
        derivation.counter_end.checked_sub(derivation.counter_start)
            == Some(u32::try_from(derivation.outputs.len())?),
        "change counter range mismatch"
    );
    if derivation.outputs.is_empty() {
        ensure!(
            derivation.maximum_change_sats == 0,
            "missing expected change derivation"
        );
        return Ok(0);
    }
    let keyset = derivation.outputs[0].keyset_id;
    let derived = PreMintSecrets::from_seed_blank(
        keyset,
        derivation.counter_start,
        seed,
        derivation.maximum_change_sats.into(),
    )?;
    ensure!(
        derived.blinded_messages() == derivation.outputs,
        "change outputs are not wallet derived"
    );
    let mut states = BTreeMap::new();
    for state in &fresh_states.states {
        ensure!(
            states.insert(state.y, state.state).is_none(),
            "duplicate change state"
        );
    }
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    for (signature, item) in signatures.iter().zip(&derived.secrets) {
        ensure!(
            signature.keyset_id == keyset && u64::from(signature.amount) > 0,
            "invalid change signature denomination"
        );
        let y = cdk::dhke::hash_to_curve(item.secret.as_bytes())?;
        ensure!(
            seen.insert(y) && states.get(&y) == Some(&State::Unspent),
            "change is not freshly unspent"
        );
        let matches = owned.iter().filter(|info| info.y == y).collect::<Vec<_>>();
        ensure!(matches.len() == 1, "owned change missing or ambiguous");
        let info = matches[0];
        let key = keys.key(mint_id, keyset, u64::from(signature.amount))?;
        let unblinded = cdk::dhke::unblind_message(&signature.c, &item.r, &key)?;
        ensure!(
            info.mint_url == derivation.mint_url
                && info.unit == CurrencyUnit::Sat
                && info.state == State::Unspent
                && info.proof.y()? == y
                && info.proof.secret == item.secret
                && info.proof.keyset_id == keyset
                && info.proof.amount == signature.amount
                && info.proof.c == unblinded,
            "recovered change differs from original signature/derivation"
        );
        let dleq = info
            .proof
            .dleq
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("change lacks DLEQ"))?;
        ensure!(dleq.r == item.r, "change blinding evidence mismatch");
        info.proof
            .verify_dleq(key)
            .map_err(|_| anyhow::anyhow!("invalid change DLEQ"))?;
        total = total
            .checked_add(u64::from(signature.amount))
            .ok_or_else(|| anyhow::anyhow!("change value overflow"))?;
    }
    ensure!(
        total <= derivation.maximum_change_sats,
        "change exceeds original input value"
    );
    Ok(total)
}

/// Attribute sorted change proofs to the already frozen per-source change
/// amounts. A denomination may span sources; these parts never create credit.
pub fn allocate_change_outputs(
    issued: &Issued,
    report: &lightning_goats_cashu_receipt::treasury::MeltData,
    outputs: &[(String, String, u64)],
) -> Result<Vec<(String, uuid::Uuid, u64)>> {
    report.validate(&issued.intent)?;
    ensure!(
        outputs.len() <= 1024 && outputs.windows(2).all(|pair| pair[0].0 < pair[1].0),
        "change outputs must be bounded, unique and sorted"
    );
    let mut remaining = report
        .sources
        .iter()
        .map(|source| source.change_sats)
        .collect::<Vec<_>>();
    let mut source = 0usize;
    let mut parts = Vec::new();
    for (y, _, amount) in outputs {
        ensure!(
            *amount > 0 && cdk::nuts::PublicKey::from_hex(y)?.to_hex() == *y,
            "invalid change output identity/value"
        );
        let mut value = *amount;
        while value > 0 {
            while source < remaining.len() && remaining[source] == 0 {
                source += 1;
            }
            ensure!(
                source < remaining.len(),
                "change outputs exceed attributed value"
            );
            let part = value.min(remaining[source]);
            parts.push((
                y.clone(),
                issued.intent.sources[source].acquisition_id,
                part,
            ));
            value -= part;
            remaining[source] -= part;
        }
    }
    ensure!(
        remaining.iter().all(|value| *value == 0),
        "change outputs omit attributed value"
    );
    Ok(parts)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cdk::lightning_invoice::{Currency, InvoiceBuilder, PaymentSecret};
    use cdk::secp256k1::{Secp256k1, SecretKey};
    use std::time::Duration;

    pub(crate) fn fixture() -> (Issued, MeltQuote, MeltQuoteBolt11Response<String>) {
        let (_, mut issued) = crate::treasury::tests::fixture();
        issued.payment_hash =
            lightning_goats_cashu_receipt::treasury::hex_digest(Sha256::digest([9u8; 32]).into());
        issued.invoice = InvoiceBuilder::new(Currency::Bitcoin)
            .description("Synthetic reconciliation".into())
            .payment_hash(issued.payment_hash.parse().unwrap())
            .payment_secret(PaymentSecret([44; 32]))
            .duration_since_epoch(Duration::from_secs(1000))
            .expiry_time(Duration::from_secs(300))
            .min_final_cltv_expiry_delta(18)
            .amount_milli_satoshis(100_000)
            .build_signed(|message| {
                Secp256k1::new()
                    .sign_ecdsa_recoverable(message, &SecretKey::from_slice(&[42; 32]).unwrap())
            })
            .unwrap()
            .to_string();
        let quote = MeltQuote {
            id: "original".into(),
            mint_url: Some("https://mint.example".parse().unwrap()),
            unit: CurrencyUnit::Sat,
            amount: 100u64.into(),
            request: issued.invoice.clone(),
            fee_reserve: 1u64.into(),
            state: MeltQuoteState::Unpaid,
            expiry: 1250,
            payment_proof: None,
            estimated_blocks: None,
            fee_index: None,
            payment_method: PaymentMethod::BOLT11,
            used_by_operation: None,
            version: 0,
        };
        let observed = MeltQuoteBolt11Response {
            quote: quote.id.clone(),
            amount: quote.amount,
            fee_reserve: quote.fee_reserve,
            state: MeltQuoteState::Paid,
            expiry: quote.expiry,
            payment_preimage: Some("09".repeat(32)),
            change: None,
            request: Some(issued.invoice.clone()),
            unit: Some(CurrencyUnit::Sat),
            method: PaymentMethod::BOLT11,
        };
        (issued, quote, observed)
    }

    #[test]
    fn paid_original_is_valid_even_after_expiry_but_not_a_settlement_report() {
        let (issued, quote, mut observed) = fixture();
        validate_paid_quote(&issued, &quote, &observed).unwrap();
        // Older mints may omit these optional echoes. The original invoice and
        // its matching preimage remain required independently of both fields.
        observed.request = None;
        observed.unit = None;
        validate_paid_quote(&issued, &quote, &observed).unwrap();
    }

    #[test]
    fn paid_label_cannot_replace_original_quote_or_preimage() {
        let (issued, quote, observed) = fixture();
        for field in 0..10 {
            let mut bad = observed.clone();
            match field {
                0 => bad.state = MeltQuoteState::Pending,
                1 => bad.quote = "replacement".into(),
                2 => bad.amount = 99u64.into(),
                3 => bad.fee_reserve = 2u64.into(),
                4 => bad.expiry += 1,
                5 => bad.request = Some("replacement invoice".into()),
                6 => bad.unit = Some(CurrencyUnit::Msat),
                7 => bad.payment_preimage = None,
                8 => bad.payment_preimage = Some("00".repeat(32)),
                _ => bad.payment_preimage = Some("invalid".into()),
            }
            assert!(validate_paid_quote(&issued, &quote, &bad).is_err());
        }
    }
    #[tokio::test]
    async fn change_requires_owned_derivation_valid_signature_and_fresh_unspent_state() -> Result<()>
    {
        use cdk::nuts::{
            BlindSignature, CheckStateResponse, Id, Keys, PreMintSecrets, Proof, ProofDleq, State,
        };
        use cdk::wallet::types::ProofInfo;
        let seed = [42; 64];
        let issuer = cdk::nuts::SecretKey::from_slice(&[7; 32])?;
        let public = issuer.public_key();
        let keys = Keys::new(std::collections::BTreeMap::from([(2u64.into(), public)]));
        let keyset = Id::v1_from_keys(&keys);
        let blank = PreMintSecrets::from_seed_blank(keyset, 10, &seed, 3u64.into())?;
        let derivation = crate::melt_derivation::MeltDerivation {
            cdk_operation: uuid::Uuid::from_u128(1),
            mint_url: "https://mint.example".parse()?,
            quote_id: "original".into(),
            counter_start: 10,
            counter_end: 10 + blank.secrets.len() as u32,
            maximum_change_sats: 3,
            outputs: blank.blinded_messages(),
        };
        let item = &blank.secrets[0];
        let signed = cdk::dhke::sign_message(&issuer, &item.blinded_message.blinded_secret)?;
        let signature = BlindSignature::new(
            2u64.into(),
            signed,
            keyset,
            &item.blinded_message.blinded_secret,
            &issuer,
        )?;
        let mut proof = Proof::new(
            2u64.into(),
            keyset,
            item.secret.clone(),
            cdk::dhke::unblind_message(&signed, &item.r, &public)?,
        );
        let dleq = signature.dleq.clone().unwrap();
        proof.dleq = Some(ProofDleq::new(dleq.e, dleq.s, item.r.clone()));
        let info = ProofInfo::new(
            proof,
            derivation.mint_url.clone(),
            State::Unspent,
            CurrencyUnit::Sat,
        )?;
        let states = CheckStateResponse {
            states: vec![(info.y, State::Unspent).into()],
        };
        let mut pinned = crate::intake::PinnedKeys::default();
        pinned.insert("fixture", keyset, 2, public)?;
        let signatures = vec![signature];
        let restoration = cdk::nuts::RestoreResponse {
            outputs: vec![derivation.outputs[0].clone()],
            signatures: signatures.clone(),
        };
        assert_eq!(
            normalize_restored_change(&derivation, &restoration)?.len(),
            1
        );
        let mut duplicate = restoration.clone();
        duplicate.outputs.push(duplicate.outputs[0].clone());
        duplicate.signatures.push(duplicate.signatures[0].clone());
        assert!(normalize_restored_change(&derivation, &duplicate).is_err());
        let mut sparse = restoration.clone();
        sparse.outputs[0] = derivation.outputs[1].clone();
        assert!(normalize_restored_change(&derivation, &sparse).is_err());
        let mut unrelated = restoration.clone();
        unrelated.outputs[0].blinded_secret =
            cdk::nuts::SecretKey::from_slice(&[99; 32])?.public_key();
        assert!(normalize_restored_change(&derivation, &unrelated).is_err());

        let owned = vec![info];
        let (issued, quote, paid) = fixture();
        let context = RecoveryContext {
            issued,
            quote,
            derivation: derivation.clone(),
        };
        let transport = QuoteTransport {
            response: paid.clone(),
            calls: Default::default(),
            states: None,
            restore: Some((derivation.outputs.clone(), restoration.clone())),
        };
        let restored = context.restore_missing_change(&transport, &paid).await?;
        let normalized = normalize_restored_change(&derivation, &restored)?;
        let reconstructed =
            reconstruct_change(&derivation, &seed, "fixture", &pinned, &normalized)?;
        assert_eq!(transport.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            verify_owned_change(
                &derivation,
                &seed,
                "fixture",
                &pinned,
                &normalized,
                &reconstructed,
                &states
            )?,
            2
        );
        let mut missing_signature_dleq = normalized.clone();
        missing_signature_dleq[0].dleq = None;
        assert!(
            reconstruct_change(
                &derivation,
                &seed,
                "fixture",
                &pinned,
                &missing_signature_dleq
            )
            .is_err()
        );

        assert_eq!(
            verify_owned_change(
                &derivation,
                &seed,
                "fixture",
                &pinned,
                &signatures,
                &owned,
                &states
            )?,
            2
        );
        assert!(
            verify_owned_change(
                &derivation,
                &[43; 64],
                "fixture",
                &pinned,
                &signatures,
                &owned,
                &states
            )
            .is_err()
        );
        for field in 0..5 {
            let mut bad = owned.clone();
            match field {
                0 => bad[0].proof.dleq = None,
                1 => bad[0].proof.c = cdk::nuts::SecretKey::from_slice(&[99; 32])?.public_key(),
                2 => bad[0].mint_url = "https://other.example".parse()?,
                3 => bad[0].state = State::Spent,
                _ => bad[0].proof.amount = 1u64.into(),
            }
            assert!(
                verify_owned_change(
                    &derivation,
                    &seed,
                    "fixture",
                    &pinned,
                    &signatures,
                    &bad,
                    &states
                )
                .is_err()
            );
        }
        let mut spent = states.clone();
        spent.states[0].state = State::Spent;
        assert!(
            verify_owned_change(
                &derivation,
                &seed,
                "fixture",
                &pinned,
                &signatures,
                &owned,
                &spent
            )
            .is_err()
        );
        let mut changed = signatures.clone();
        changed[0].c = cdk::nuts::SecretKey::from_slice(&[99; 32])?.public_key();
        assert!(
            verify_owned_change(
                &derivation,
                &seed,
                "fixture",
                &pinned,
                &changed,
                &owned,
                &states
            )
            .is_err()
        );
        Ok(())
    }
    #[derive(Clone, Debug)]
    pub(crate) struct QuoteTransport {
        pub(crate) response: MeltQuoteBolt11Response<String>,
        pub(crate) calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        pub(crate) states: Option<cdk::nuts::CheckStateResponse>,
        pub(crate) restore: Option<(Vec<cdk::nuts::BlindedMessage>, cdk::nuts::RestoreResponse)>,
    }
    #[async_trait::async_trait]
    impl cdk_http_client::Transport for QuoteTransport {
        fn with_proxy(
            &mut self,
            _: url::Url,
            _: Option<&str>,
            _: bool,
        ) -> Result<(), cdk_http_client::HttpError> {
            panic!("recovery must not configure proxy")
        }
        async fn ws_connect(
            &self,
            _: &str,
            _: &[(&str, &str)],
        ) -> Result<
            (
                cdk_http_client::ws::WsSender,
                cdk_http_client::ws::WsReceiver,
            ),
            cdk_http_client::ws::WsError,
        > {
            panic!("recovery must not open WebSocket")
        }
        async fn http_get<R: serde::de::DeserializeOwned>(
            &self,
            url: url::Url,
            auth: Option<cdk::nuts::AuthToken>,
        ) -> Result<R, cdk_http_client::HttpError> {
            assert_eq!(
                url.as_str(),
                "https://mint.example/v1/melt/quote/bolt11/original"
            );
            assert!(auth.is_none());
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            serde_json::from_value(serde_json::to_value(&self.response).unwrap())
                .map_err(|_| cdk_http_client::HttpError::Other("fixture decode".into()))
        }
        async fn http_get_raw(
            &self,
            _: url::Url,
            _: Option<cdk::nuts::AuthToken>,
        ) -> Result<cdk_http_client::RawResponse, cdk_http_client::HttpError> {
            panic!("unexpected raw GET")
        }
        async fn http_post<P: serde::Serialize + Send + Sync, R: serde::de::DeserializeOwned>(
            &self,
            url: url::Url,
            auth: Option<cdk::nuts::AuthToken>,
            payload: &P,
        ) -> Result<R, cdk_http_client::HttpError> {
            if url.path() == "/v1/checkstate" {
                let states = self.states.as_ref().expect("unexpected state lookup");
                assert!(auth.is_none());
                assert_eq!(url.as_str(), "https://mint.example/v1/checkstate");
                let request: cdk::nuts::CheckStateRequest =
                    serde_json::from_value(serde_json::to_value(payload).unwrap()).unwrap();
                assert_eq!(
                    request.ys,
                    states
                        .states
                        .iter()
                        .map(|state| state.y)
                        .collect::<Vec<_>>()
                );
                self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                return serde_json::from_value(serde_json::to_value(states).unwrap())
                    .map_err(|_| cdk_http_client::HttpError::Other("fixture state decode".into()));
            }
            let (expected, response) = self
                .restore
                .as_ref()
                .expect("recovery must never submit payment");
            assert_eq!(url.as_str(), "https://mint.example/v1/restore");
            assert!(auth.is_none());
            let request: cdk::nuts::RestoreRequest =
                serde_json::from_value(serde_json::to_value(payload).unwrap()).unwrap();
            assert_eq!(&request.outputs, expected);
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            serde_json::from_value(serde_json::to_value(response).unwrap())
                .map_err(|_| cdk_http_client::HttpError::Other("fixture restore decode".into()))
        }
        async fn http_post_form_raw<P: serde::Serialize + Send + Sync>(
            &self,
            _: url::Url,
            _: Option<cdk::nuts::AuthToken>,
            _: &P,
        ) -> Result<cdk_http_client::RawResponse, cdk_http_client::HttpError> {
            panic!("recovery must never submit form")
        }
    }

    #[tokio::test]
    async fn original_quote_poll_is_read_only_and_nonpaid_never_authorizes_replacement()
    -> Result<()> {
        let (issued, quote, response) = fixture();
        let context = RecoveryContext {
            issued,
            derivation: crate::melt_derivation::MeltDerivation {
                cdk_operation: uuid::Uuid::from_u128(1),
                mint_url: quote.mint_url.clone().unwrap(),
                quote_id: quote.id.clone(),
                counter_start: 0,
                counter_end: 0,
                maximum_change_sats: 0,
                outputs: vec![],
            },
            quote,
        };
        let mut transport = QuoteTransport {
            response,
            calls: Default::default(),
            states: None,
            restore: None,
        };
        assert!(context.poll_paid(&transport).await?.is_some());
        for state in [MeltQuoteState::Unpaid, MeltQuoteState::Pending] {
            transport.response.state = state;
            transport.response.payment_preimage = None;
            assert!(context.poll_paid(&transport).await?.is_none());
        }
        transport.response.amount = 99u64.into();
        assert!(context.poll_paid(&transport).await.is_err());
        assert_eq!(transport.calls.load(std::sync::atomic::Ordering::SeqCst), 4);
        let mut changed = context;
        changed.quote.id = "replacement".into();
        assert!(changed.poll_paid(&transport).await.is_err());
        assert_eq!(transport.calls.load(std::sync::atomic::Ordering::SeqCst), 4);
        Ok(())
    }

    #[tokio::test]
    async fn restored_mint_observation_binds_original_quote_and_preserves_uncertain_state()
    -> Result<()> {
        let (issued, quote, response) = fixture();
        let mint = quote.mint_url.clone().unwrap();
        let mut historical_quote = serde_json::to_value(quote)?;
        historical_quote.as_object_mut().unwrap().retain(|key, _| {
            [
                "id",
                "mint_url",
                "unit",
                "amount",
                "request",
                "fee_reserve",
                "expiry",
                "payment_method",
                "state",
            ]
            .contains(&key.as_str())
        });
        let selection: crate::restore_mint::Selection =
            serde_json::from_value(serde_json::json!({
                "version": 1,
                "purpose": "cashu_restore_mint_melt",
                "checkpoint_id": uuid::Uuid::from_u128(1),
                "staged_receipt_sha256": "11".repeat(32),
                "issued": issued,
                "operation_id": uuid::Uuid::from_u128(2),
                "quote": historical_quote,
            }))?;
        let mut transport = QuoteTransport {
            response,
            calls: Default::default(),
            states: None,
            restore: None,
        };
        let digest = "22".repeat(32);
        assert!(
            selection
                .observe(&"https://different.example".parse()?, &digest, &transport)
                .await
                .is_err()
        );
        assert_eq!(transport.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        let paid = selection.observe(&mint, &digest, &transport).await?;
        assert_eq!(paid["paid_preimage_verified"], true);
        assert_eq!(paid["selection_sha256"], digest);
        assert_eq!(paid["resume_authorized"], false);
        transport.response.state = MeltQuoteState::Pending;
        assert!(selection.observe(&mint, &digest, &transport).await.is_err());
        transport.response.payment_preimage = None;
        transport.response.change = None;
        let pending = selection.observe(&mint, &digest, &transport).await?;
        assert_eq!(pending["paid_preimage_verified"], false);
        assert_eq!(pending["resume_authorized"], false);
        assert_eq!(transport.calls.load(std::sync::atomic::Ordering::SeqCst), 3);
        Ok(())
    }
    #[test]
    fn change_denominations_split_deterministically_across_original_sources() -> Result<()> {
        let (mut issued, _, _) = fixture();
        issued.intent.input_sats = 4;
        issued.intent.deposit_sats = 1;
        issued.intent.maximum_fee_sats = 0;
        issued.intent.sources[0].input_sats = 2;
        let mut second = issued.intent.sources[0].clone();
        second.ledger_id += 1;
        second.acquisition_id = uuid::Uuid::from_u128(99);
        issued.intent.sources.push(second);
        let report = lightning_goats_cashu_receipt::treasury::MeltData {
            version: 1,
            intent_id: issued.intent.id,
            intent_digest: lightning_goats_cashu_receipt::treasury::hex_digest(
                issued.intent.digest()?,
            ),
            quote_id: "original".into(),
            operation_evidence_digest: "aa".repeat(32),
            sources: issued.intent.allocate_result(0, 3)?,
        };
        let mut ys = [
            cdk::dhke::hash_to_curve(b"first")?.to_hex(),
            cdk::dhke::hash_to_curve(b"second")?.to_hex(),
        ];
        ys.sort();
        let outputs = vec![
            (ys[0].clone(), ys[0].clone(), 2),
            (ys[1].clone(), ys[1].clone(), 1),
        ];
        let parts = allocate_change_outputs(&issued, &report, &outputs)?;
        assert_eq!(
            parts,
            vec![
                (ys[0].clone(), issued.intent.sources[0].acquisition_id, 1),
                (ys[0].clone(), issued.intent.sources[1].acquisition_id, 1),
                (ys[1].clone(), issued.intent.sources[1].acquisition_id, 1)
            ]
        );
        assert!(allocate_change_outputs(&issued, &report, &outputs[..1]).is_err());
        let mut excessive = outputs.clone();
        excessive[1].2 = 2;
        assert!(allocate_change_outputs(&issued, &report, &excessive).is_err());
        Ok(())
    }
}
