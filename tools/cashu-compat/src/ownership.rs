//! Verify acquired outputs from frozen CDK derivation material, not status labels.
use crate::{intake::PinnedKeys, journal::Journal};
use anyhow::{Result, ensure};
use cdk::{
    Amount,
    mint_url::MintUrl,
    nuts::{
        BlindedMessage, CheckStateResponse, CurrencyUnit, Id, PreMintSecrets, State, SwapRequest,
    },
    wallet::types::{
        OperationData, ProofInfo, ReceiveSagaState, SwapSagaState, WalletSaga, WalletSagaState,
    },
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// Persist this with the original request before any swap is submitted.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SwapDerivation {
    pub cdk_operation: Uuid,
    pub mint_url: MintUrl,
    pub keyset_id: Id,
    pub counter_start: u32,
    pub counter_end: u32,
    pub outputs: Vec<BlindedMessage>,
}
impl SwapDerivation {
    pub(crate) fn output_ys(&self, seed: &[u8; 64]) -> Result<Vec<cdk::nuts::PublicKey>> {
        let count = self
            .counter_end
            .checked_sub(self.counter_start)
            .ok_or_else(|| anyhow::anyhow!("invalid derivation range"))?;
        ensure!(
            count > 0 && count <= 1024 && usize::try_from(count)? == self.outputs.len(),
            "invalid derivation range"
        );
        let derived = PreMintSecrets::restore_batch(
            self.keyset_id,
            seed,
            self.counter_start,
            self.counter_end,
        )?;
        let by_blinded = derived
            .secrets
            .into_iter()
            .map(|item| (item.blinded_message.blinded_secret.to_bytes(), item.secret))
            .collect::<BTreeMap<_, _>>();
        self.outputs
            .iter()
            .map(|output| {
                let secret = by_blinded
                    .get(&output.blinded_secret.to_bytes())
                    .ok_or_else(|| anyhow::anyhow!("output is not wallet derived"))?;
                Ok(cdk::dhke::hash_to_curve(secret.as_bytes())?)
            })
            .collect()
    }

    pub fn capture(saga: &WalletSaga, request: &SwapRequest) -> Result<Self> {
        ensure!(
            !saga.id.is_nil()
                && saga.unit == CurrencyUnit::Sat
                && saga.state == WalletSagaState::Receive(ReceiveSagaState::SwapRequested),
            "invalid receive recovery scope"
        );
        let data = match &saga.data {
            OperationData::Receive(data) => data,
            _ => anyhow::bail!("not a receive operation"),
        };
        ensure!(
            saga.amount == request.input_amount()? && data.amount == Some(saga.amount),
            "receive input amount changed"
        );
        let start = data
            .counter_start
            .ok_or_else(|| anyhow::anyhow!("missing derivation start"))?;
        let end = data
            .counter_end
            .ok_or_else(|| anyhow::anyhow!("missing derivation end"))?;
        let outputs = data
            .blinded_messages
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing frozen outputs"))?;
        ensure!(
            outputs == request.outputs()
                && !outputs.is_empty()
                && outputs.len() <= 1024
                && end.checked_sub(start) == Some(u32::try_from(outputs.len())?),
            "unsupported or conflicting derivation range"
        );
        let keyset_id = outputs[0].keyset_id;
        ensure!(
            outputs
                .iter()
                .all(|output| output.keyset_id == keyset_id && output.amount > Amount::ZERO),
            "unsupported output keyset/amount"
        );
        Ok(Self {
            cdk_operation: saga.id,
            mint_url: saga.mint_url.clone(),
            keyset_id,
            counter_start: start,
            counter_end: end,
            outputs: outputs.clone(),
        })
    }

    /// Capture only an already requested standalone split. This is derivation
    /// evidence, not submission authority or permission to create donor credit.
    pub fn capture_split(saga: &WalletSaga, request: &SwapRequest, fee_sats: u64) -> Result<Self> {
        ensure!(
            !saga.id.is_nil()
                && saga.unit == CurrencyUnit::Sat
                && saga.state == WalletSagaState::Swap(SwapSagaState::SwapRequested),
            "invalid split recovery scope"
        );
        let data = match &saga.data {
            OperationData::Swap(data) => data,
            _ => anyhow::bail!("not a split swap operation"),
        };
        ensure!(
            saga.amount == request.input_amount()?
                && data.input_amount == saga.amount
                && data.output_amount == request.output_amount()?
                && u64::from(data.output_amount).checked_add(fee_sats)
                    == Some(u64::from(data.input_amount)),
            "split amounts or fee changed"
        );
        let start = data
            .counter_start
            .ok_or_else(|| anyhow::anyhow!("missing derivation start"))?;
        let end = data
            .counter_end
            .ok_or_else(|| anyhow::anyhow!("missing derivation end"))?;
        let outputs = data
            .blinded_messages
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing frozen outputs"))?;
        ensure!(
            outputs == request.outputs()
                && !outputs.is_empty()
                && outputs.len() <= 1024
                && end.checked_sub(start) == Some(u32::try_from(outputs.len())?),
            "unsupported or conflicting derivation range"
        );
        let unique = outputs
            .iter()
            .map(|o| o.blinded_secret.to_bytes())
            .collect::<BTreeSet<_>>();
        ensure!(unique.len() == outputs.len(), "duplicate split output");
        let keyset_id = outputs[0].keyset_id;
        ensure!(
            outputs
                .iter()
                .all(|output| output.keyset_id == keyset_id && output.amount > Amount::ZERO),
            "unsupported output keyset/amount"
        );
        Ok(Self {
            cdk_operation: saga.id,
            mint_url: saga.mint_url.clone(),
            keyset_id,
            counter_start: start,
            counter_end: end,
            outputs: outputs.clone(),
        })
    }
}

/// Sealed result: constructing a balance or a Completed saga cannot manufacture it.
pub struct OwnedAcquisition {
    acquisition_id: Uuid,
    net: u64,
    fee: u64,
    evidence: Vec<u8>,
    outputs: Vec<(String, String, u64)>,
}
impl OwnedAcquisition {
    pub(crate) fn commit(self, journal: &mut Journal, recheck_hold: bool) -> Result<()> {
        journal.record_acquired_outputs(
            self.acquisition_id,
            self.net,
            self.fee,
            &self.evidence,
            &self.outputs,
            recheck_hold,
        )
    }
}

/// Caller supplies fresh authoritative mint state for these exact output Ys.
/// The service must reconcile backup epochs before obtaining this evidence.
#[allow(clippy::too_many_arguments)]
pub fn verify_owned_outputs(
    acquisition_id: Uuid,
    journal: &Journal,
    derivation: &SwapDerivation,
    seed: &[u8; 64],
    mint_id: &str,
    keys: &PinnedKeys,
    owned: &[ProofInfo],
    fresh_states: &CheckStateResponse,
) -> Result<OwnedAcquisition> {
    ensure!(
        !acquisition_id.is_nil() && !derivation.cdk_operation.is_nil(),
        "missing acquisition identity"
    );
    ensure!(
        journal.derivation(acquisition_id)? == *derivation,
        "derivation not frozen for this acquisition"
    );
    ensure!(
        journal.mint_id(acquisition_id)? == mint_id,
        "acquisition mint identity changed"
    );
    let original = journal
        .prepared_request(acquisition_id)?
        .ok_or_else(|| anyhow::anyhow!("original swap missing"))?;
    let request: SwapRequest =
        serde_json::from_slice(&original).map_err(|_| anyhow::anyhow!("invalid original swap"))?;
    let fee = journal.verify_swap_inputs(acquisition_id, &request)?;
    ensure!(
        request.outputs() == &derivation.outputs,
        "frozen output binding changed"
    );
    let verified = verify_derived_outputs(derivation, seed, mint_id, keys, owned, fresh_states)?;
    let total = u64::from(request.output_amount()?);
    let evidence = serde_json::to_vec(&(
        1,
        acquisition_id.to_string(),
        derivation.cdk_operation.to_string(),
        Sha256::digest(&original).to_vec(),
        &verified,
    ))?;
    Ok(OwnedAcquisition {
        acquisition_id,
        net: total,
        fee,
        evidence,
        outputs: verified,
    })
}

/// Verify exact seed-derived ownership without creating an acquisition or credit.
/// Both acquisition and split reconciliation use the same crypto checks.
pub(crate) fn verify_derived_outputs(
    derivation: &SwapDerivation,
    seed: &[u8; 64],
    mint_id: &str,
    keys: &PinnedKeys,
    owned: &[ProofInfo],
    fresh_states: &CheckStateResponse,
) -> Result<Vec<(String, String, u64)>> {
    let count = derivation
        .counter_end
        .checked_sub(derivation.counter_start)
        .ok_or_else(|| anyhow::anyhow!("invalid derivation range"))?;
    ensure!(
        count > 0 && count <= 1024 && usize::try_from(count)? == derivation.outputs.len(),
        "unbounded derivation range"
    );
    let derived = PreMintSecrets::restore_batch(
        derivation.keyset_id,
        seed,
        derivation.counter_start,
        derivation.counter_end,
    )?;
    let mut premints = BTreeMap::new();
    for item in derived.secrets {
        ensure!(
            premints
                .insert(item.blinded_message.blinded_secret.to_bytes(), item)
                .is_none(),
            "duplicate derived output"
        );
    }
    let mut expected = BTreeSet::new();
    let mut verified = Vec::new();
    let mut total = 0_u64;
    for output in &derivation.outputs {
        ensure!(
            output.keyset_id == derivation.keyset_id && output.amount > Amount::ZERO,
            "output keyset changed"
        );
        let item = premints
            .remove(&output.blinded_secret.to_bytes())
            .ok_or_else(|| anyhow::anyhow!("output not derived by wallet"))?;
        let y = cdk::dhke::hash_to_curve(item.secret.as_bytes())?;
        ensure!(expected.insert(y.to_bytes()), "duplicate output identity");
        let matches = owned
            .iter()
            .filter(|proof| proof.y == y)
            .collect::<Vec<_>>();
        ensure!(matches.len() == 1, "owned output missing or ambiguous");
        let info = matches[0];
        ensure!(
            info.proof.y()? == y
                && info.proof.secret == item.secret
                && info.mint_url == derivation.mint_url
                && info.unit == CurrencyUnit::Sat
                && info.state == State::Unspent
                && info.proof.amount == output.amount
                && info.proof.keyset_id == output.keyset_id,
            "owned output provenance changed"
        );
        let dleq = info
            .proof
            .dleq
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("owned output requires DLEQ"))?;
        ensure!(dleq.r == item.r, "owned output blinding evidence changed");
        info.proof
            .verify_dleq(keys.key(mint_id, output.keyset_id, u64::from(output.amount))?)
            .map_err(|_| anyhow::anyhow!("invalid owned output DLEQ"))?;
        total = total
            .checked_add(u64::from(output.amount))
            .ok_or_else(|| anyhow::anyhow!("owned amount overflow"))?;
        verified.push((y.to_hex(), info.proof.c.to_hex(), u64::from(output.amount)));
    }
    ensure!(premints.is_empty(), "unexpected unused derivations");
    let mut states = BTreeMap::new();
    for state in &fresh_states.states {
        ensure!(
            states.insert(state.y.to_bytes(), state.state).is_none(),
            "duplicate mint output state"
        );
    }
    ensure!(
        states.len() == expected.len()
            && expected
                .iter()
                .all(|y| states.get(y) == Some(&State::Unspent)),
        "output state not authoritatively unspent"
    );
    verified.sort();
    Ok(verified)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{Admission, Transport, ValidatedIntake};
    use cdk::{
        dhke,
        nuts::{BlindSignature, Proof, ProofDleq, SecretKey},
        secret::Secret,
        wallet::types::ReceiveOperationData,
    };
    use std::str::FromStr;

    #[test]
    fn split_capture_requires_original_swap_state_exact_fee_and_frozen_derivation() -> Result<()> {
        use cdk::wallet::types::SwapOperationData;
        let seed = [41; 64];
        let keyset = Id::from_str("00deadbeef123456")?;
        let secret = Secret::new("split-capture-fixture");
        let input = Proof::new(
            Amount::from(2),
            keyset,
            secret.clone(),
            dhke::hash_to_curve(secret.as_bytes())?,
        );
        let premints = PreMintSecrets::restore_batch(keyset, &seed, 0, 1)?;
        let mut output = premints.secrets[0].blinded_message.clone();
        output.amount = Amount::from(1);
        let request = SwapRequest::new(vec![input], vec![output.clone()]);
        let saga = WalletSaga::new(
            Uuid::from_u128(44),
            WalletSagaState::Swap(SwapSagaState::SwapRequested),
            Amount::from(2),
            MintUrl::from_str("https://mint.example")?,
            CurrencyUnit::Sat,
            OperationData::Swap(SwapOperationData {
                input_amount: Amount::from(2),
                output_amount: Amount::from(1),
                counter_start: Some(0),
                counter_end: Some(1),
                blinded_messages: Some(vec![output]),
            }),
        );
        let retained = SwapDerivation::capture_split(&saga, &request, 1)?;
        assert_eq!(retained.cdk_operation, saga.id);
        assert_eq!(
            retained.output_ys(&seed)?,
            vec![dhke::hash_to_curve(premints.secrets[0].secret.as_bytes())?]
        );
        assert!(SwapDerivation::capture(&saga, &request).is_err());
        assert!(SwapDerivation::capture_split(&saga, &request, 0).is_err());
        for change in 0..7 {
            let mut invalid = saga.clone();
            match change {
                0 => invalid.state = WalletSagaState::Swap(SwapSagaState::ProofsReserved),
                1 => invalid.id = Uuid::nil(),
                2 => invalid.amount = Amount::from(3),
                _ => {
                    if let OperationData::Swap(ref mut data) = invalid.data {
                        match change {
                            3 => data.input_amount = Amount::from(3),
                            4 => data.output_amount = Amount::from(2),
                            5 => data.counter_end = Some(2),
                            _ => data.blinded_messages = None,
                        }
                    }
                }
            }
            assert!(
                SwapDerivation::capture_split(&invalid, &request, 1).is_err(),
                "mutation {change}"
            );
        }
        Ok(())
    }

    #[test]
    fn recovery_requires_exact_owned_outputs_and_fresh_unspent_evidence() -> Result<()> {
        let directory = crate::test_directory()?;
        let mut journal =
            Journal::open(&directory.path().join("journal.sqlite"), Uuid::new_v4(), 8)?;
        let seed = [42; 64];
        let issuer = SecretKey::from_slice(&[7; 32])?;
        let keyset = Id::from_str("00deadbeef123456")?;
        let input_secret = Secret::new("fixture-input");
        let input = Proof::new(
            Amount::from(8),
            keyset,
            input_secret.clone(),
            dhke::sign_message(&issuer, &dhke::hash_to_curve(input_secret.as_bytes())?)?,
        );
        let id = match journal.admit(&ValidatedIntake {
            mint_id: "fixture",
            policy_version: "v1",
            transport: Transport::Cashu,
            source_ref: "fixture-request",
            source_author: None,
            proofs: &[input.clone()],
        })? {
            Admission::New(id) => id,
            _ => panic!("new fixture expected"),
        };
        let derived = PreMintSecrets::restore_batch(keyset, &seed, 0, 1)?;
        let premint = &derived.secrets[0];
        let mut output = premint.blinded_message.clone();
        output.amount = Amount::from(8);
        let request = SwapRequest::new(vec![input], vec![output.clone()]);
        let mint = MintUrl::from_str("https://mint.example")?;
        let saga = WalletSaga::new(
            Uuid::new_v4(),
            WalletSagaState::Receive(ReceiveSagaState::SwapRequested),
            Amount::from(8),
            mint.clone(),
            CurrencyUnit::Sat,
            OperationData::Receive(ReceiveOperationData {
                token: None,
                counter_start: Some(0),
                counter_end: Some(1),
                amount: Some(Amount::from(8)),
                blinded_messages: Some(vec![output.clone()]),
            }),
        );
        let binding = SwapDerivation::capture(&saga, &request)?;
        journal.prepare(id, &serde_json::to_vec(&request)?)?;
        journal.start_acquisition(id)?;
        journal.record_derivation(id, &binding)?;
        let blinded_signature = dhke::sign_message(&issuer, &output.blinded_secret)?;
        let signature = BlindSignature::new(
            output.amount,
            blinded_signature,
            keyset,
            &output.blinded_secret,
            &issuer,
        )?;
        let unblinded =
            dhke::unblind_message(&blinded_signature, &premint.r, &issuer.public_key())?;
        let mut proof = Proof::new(output.amount, keyset, premint.secret.clone(), unblinded);
        let dleq = signature.dleq.expect("fixture mint supplies DLEQ");
        proof.dleq = Some(ProofDleq::new(dleq.e, dleq.s, premint.r.clone()));
        let info = ProofInfo::new(proof, mint, State::Unspent, CurrencyUnit::Sat)?;
        let mut keys = PinnedKeys::default();
        keys.insert("fixture", keyset, 8, issuer.public_key())?;
        let states = CheckStateResponse {
            states: vec![(info.y, State::Unspent).into()],
        };
        assert!(
            verify_owned_outputs(
                id,
                &journal,
                &binding,
                &seed,
                "fixture",
                &keys,
                &[],
                &states
            )
            .is_err()
        );
        assert!(
            verify_owned_outputs(
                id,
                &journal,
                &binding,
                &[1; 64],
                "fixture",
                &keys,
                std::slice::from_ref(&info),
                &states
            )
            .is_err()
        );
        let spent = CheckStateResponse {
            states: vec![(info.y, State::Spent).into()],
        };
        assert!(
            verify_owned_outputs(
                id,
                &journal,
                &binding,
                &seed,
                "fixture",
                &keys,
                std::slice::from_ref(&info),
                &spent
            )
            .is_err()
        );
        let mut corrupt = info.clone();
        corrupt.proof.c = SecretKey::from_slice(&[9; 32])?.public_key();
        assert!(
            verify_owned_outputs(
                id,
                &journal,
                &binding,
                &seed,
                "fixture",
                &keys,
                &[corrupt],
                &states
            )
            .is_err()
        );
        assert!(journal.export_receipt(id).is_err());
        verify_owned_outputs(
            id,
            &journal,
            &binding,
            &seed,
            "fixture",
            &keys,
            &[info],
            &states,
        )?
        .commit(&mut journal, false)?;
        let receipt = journal.export_receipt(id)?;
        assert_eq!(receipt.net_sats, 8);
        assert_eq!(receipt.receive_fee_sats, 0);
        assert_eq!(receipt.credit_pool, "herd");
        Ok(())
    }
}
