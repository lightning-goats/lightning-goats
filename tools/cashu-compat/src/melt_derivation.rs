//! Validate recovery material before a treasury melt can cross the transport.
use anyhow::{Result, ensure};
use cdk::{
    Amount,
    mint_url::MintUrl,
    nuts::{BlindedMessage, CurrencyUnit, MeltRequest, PreMintSecrets},
    wallet::types::{MeltSagaState, OperationData, WalletSaga, WalletSagaState},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MeltDerivation {
    pub cdk_operation: Uuid,
    pub mint_url: MintUrl,
    pub quote_id: String,
    pub counter_start: u32,
    pub counter_end: u32,
    pub maximum_change_sats: u64,
    pub outputs: Vec<BlindedMessage>,
}
impl MeltDerivation {
    /// This verifies local recovery data, not payment or authority to submit.
    /// The caller must additionally match the frozen treasury quote/proof holds.
    pub fn capture(
        saga: &WalletSaga,
        request: &MeltRequest<String>,
        input_fee_sats: u64,
        seed: &[u8; 64],
    ) -> Result<Self> {
        ensure!(
            !saga.id.is_nil()
                && saga.unit == CurrencyUnit::Sat
                && saga.state == WalletSagaState::Melt(MeltSagaState::MeltRequested),
            "invalid melt recovery scope"
        );
        let data = match &saga.data {
            OperationData::Melt(data) => data,
            _ => anyhow::bail!("not a melt operation"),
        };
        ensure!(
            !data.quote_id.is_empty()
                && data.quote_id.len() <= 256
                && &data.quote_id == request.quote_id()
                && saga.quote_id.as_ref() == Some(&data.quote_id)
                && saga.amount == data.amount,
            "melt quote identity changed"
        );
        ensure!(
            !request.inputs().is_empty() && request.inputs().len() <= 1024,
            "invalid melt input count"
        );
        let mut ys = BTreeSet::new();
        let mut input_sats = 0u64;
        for proof in request.inputs() {
            ensure!(
                proof.amount > Amount::ZERO && ys.insert(proof.y()?),
                "invalid or duplicate melt input"
            );
            input_sats = input_sats
                .checked_add(u64::from(proof.amount))
                .ok_or_else(|| anyhow::anyhow!("melt input overflow"))?;
        }
        let retained = data
            .final_proof_ys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing final melt inputs"))?;
        ensure!(
            retained.len() == ys.len() && retained.iter().copied().collect::<BTreeSet<_>>() == ys,
            "melt inputs differ from retained saga"
        );
        let maximum_change_sats = input_sats
            .checked_sub(u64::from(data.amount))
            .and_then(|n| n.checked_sub(input_fee_sats))
            .ok_or_else(|| anyhow::anyhow!("melt inputs cannot cover deposit and input fees"))?;
        ensure!(
            maximum_change_sats >= u64::from(data.fee_reserve),
            "melt inputs cannot cover fee reserve"
        );
        ensure!(
            data.change_amount.map(u64::from).unwrap_or(0) == maximum_change_sats,
            "melt recovery change amount differs"
        );
        ensure!(
            &data.change_blinded_messages == request.outputs(),
            "melt recovery outputs differ"
        );
        let outputs = request.outputs().clone().unwrap_or_default();
        let start = data
            .counter_start
            .ok_or_else(|| anyhow::anyhow!("missing melt counter start"))?;
        let end = data
            .counter_end
            .ok_or_else(|| anyhow::anyhow!("missing melt counter end"))?;
        ensure!(
            outputs.len() <= 1024 && end.checked_sub(start) == Some(u32::try_from(outputs.len())?),
            "invalid melt recovery counter range"
        );
        if maximum_change_sats == 0 {
            ensure!(outputs.is_empty(), "unexpected melt change outputs");
        } else {
            ensure!(!outputs.is_empty(), "missing melt change outputs");
            let keyset = outputs[0].keyset_id;
            let expected =
                PreMintSecrets::from_seed_blank(keyset, start, seed, maximum_change_sats.into())?;
            ensure!(
                expected.blinded_messages() == outputs,
                "melt change is not derived from this wallet"
            );
        }
        Ok(Self {
            cdk_operation: saga.id,
            mint_url: saga.mint_url.clone(),
            quote_id: data.quote_id.clone(),
            counter_start: start,
            counter_end: end,
            maximum_change_sats,
            outputs,
        })
    }
}

/// Validate the exact melt inputs against pinned mint keys and a fresh NUT-07
/// response fetched by the trusted transport. A local balance/state is insufficient.
/// This check does not replace durable proof/source reservation or invoice scope.
pub fn verify_melt_inputs(
    request: &MeltRequest<String>,
    mint_id: &str,
    mint_url: &MintUrl,
    owned: &[cdk::wallet::types::ProofInfo],
    keys: &crate::intake::PinnedKeys,
    fresh_states: &cdk::nuts::CheckStateResponse,
) -> Result<()> {
    verify_owned_inputs(
        request.inputs(),
        mint_id,
        mint_url,
        owned,
        keys,
        fresh_states,
    )
}

/// Shared mint-bound proof and fresh-state verification for melts and splits.
pub(crate) fn verify_owned_inputs(
    inputs: &[cdk::nuts::Proof],
    mint_id: &str,
    mint_url: &MintUrl,
    owned: &[cdk::wallet::types::ProofInfo],
    keys: &crate::intake::PinnedKeys,
    fresh_states: &cdk::nuts::CheckStateResponse,
) -> Result<()> {
    use cdk::nuts::State;
    ensure!(
        !inputs.is_empty() && inputs.len() <= 1024,
        "invalid melt proof count"
    );
    ensure!(
        fresh_states.states.len() == inputs.len(),
        "melt state response count mismatch"
    );
    let mut states = std::collections::BTreeMap::new();
    for state in &fresh_states.states {
        ensure!(
            states.insert(state.y, state.state).is_none(),
            "duplicate mint input state"
        );
    }
    let mut expected = BTreeSet::new();
    for proof in inputs {
        let y = proof.y()?;
        ensure!(
            proof.amount > Amount::ZERO && expected.insert(y),
            "invalid or duplicate melt proof"
        );
        ensure!(
            states.get(&y) == Some(&State::Unspent),
            "melt input is not authoritatively unspent"
        );
        let matches = owned.iter().filter(|info| info.y == y).collect::<Vec<_>>();
        ensure!(matches.len() == 1, "owned melt input missing or ambiguous");
        let info = matches[0];
        ensure!(
            &info.mint_url == mint_url
                && info.unit == CurrencyUnit::Sat
                && info.proof.y()? == y
                && info.proof.secret == proof.secret
                && info.proof.c == proof.c
                && info.proof.keyset_id == proof.keyset_id
                && info.proof.amount == proof.amount,
            "melt input differs from wallet proof"
        );
        ensure!(info.proof.dleq.is_some(), "owned melt input lacks DLEQ");
        info.proof
            .verify_dleq(keys.key(mint_id, proof.keyset_id, u64::from(proof.amount))?)
            .map_err(|_| anyhow::anyhow!("invalid melt input DLEQ"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cdk::{
        nuts::{Id, Keys, Proof, SecretKey},
        secret::Secret,
        wallet::types::MeltOperationData,
    };
    use std::collections::BTreeMap;

    fn fixture(change: u64) -> (WalletSaga, MeltRequest<String>, [u8; 64]) {
        let seed = [42; 64];
        let key = SecretKey::from_slice(&[7; 32]).unwrap();
        let keys = Keys::new(BTreeMap::from([(Amount::from(8), key.public_key())]));
        let id = Id::v1_from_keys(&keys);
        let proof = Proof::new(
            8u64.into(),
            id,
            Secret::new("synthetic-input"),
            key.public_key(),
        );
        let outputs = if change == 0 {
            None
        } else {
            Some(
                PreMintSecrets::from_seed_blank(id, 12, &seed, change.into())
                    .unwrap()
                    .blinded_messages(),
            )
        };
        let end = 12 + outputs.as_ref().map_or(0, |v| v.len() as u32);
        let data = MeltOperationData {
            quote_id: "original-quote".into(),
            amount: (8 - change).into(),
            fee_reserve: 0u64.into(),
            counter_start: Some(12),
            counter_end: Some(end),
            change_amount: (change > 0).then_some(change.into()),
            metadata: Default::default(),
            final_proof_ys: Some(vec![proof.y().unwrap()]),
            change_blinded_messages: outputs.clone(),
        };
        let saga = WalletSaga::new(
            Uuid::from_u128(1),
            WalletSagaState::Melt(MeltSagaState::MeltRequested),
            data.amount,
            "https://mint.example".parse().unwrap(),
            CurrencyUnit::Sat,
            OperationData::Melt(data),
        );
        (
            saga,
            MeltRequest::new("original-quote".into(), vec![proof], outputs),
            seed,
        )
    }

    #[test]
    fn original_change_is_wallet_derived_and_zero_change_needs_no_outputs() {
        for change in [0, 1, 2, 3, 7] {
            let (saga, request, seed) = fixture(change);
            let retained = MeltDerivation::capture(&saga, &request, 0, &seed).unwrap();
            assert_eq!(retained.maximum_change_sats, change);
            assert_eq!(retained.cdk_operation, saga.id);
            if change > 0 {
                assert!(MeltDerivation::capture(&saga, &request, 0, &[43; 64]).is_err());
            }
        }
    }

    #[test]
    fn conflicting_recovery_material_and_duplicate_inputs_are_refused() {
        let (saga, request, seed) = fixture(3);
        for field in 0..8 {
            let mut changed = saga.clone();
            let OperationData::Melt(data) = &mut changed.data else {
                unreachable!()
            };
            match field {
                0 => data.counter_start = None,
                1 => data.counter_end = Some(999),
                2 => data.final_proof_ys = None,
                3 => data.final_proof_ys.as_mut().unwrap().clear(),
                4 => data.change_amount = Some(2u64.into()),
                5 => data.quote_id = "replacement".into(),
                6 => data.change_blinded_messages = None,
                _ => data.fee_reserve = 4u64.into(),
            }
            assert!(MeltDerivation::capture(&changed, &request, 0, &seed).is_err());
        }
        let mut duplicate = request.clone();
        duplicate.inputs_mut().push(request.inputs()[0].clone());
        assert!(MeltDerivation::capture(&saga, &duplicate, 0, &seed).is_err());
        assert!(MeltDerivation::capture(&saga, &request, 1, &seed).is_err());
    }
}
