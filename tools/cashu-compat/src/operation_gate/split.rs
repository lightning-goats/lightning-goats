//! Explicit split authority on the shared writer; recovery never permits POST swap.
use super::*;
use crate::journal::treasury_split::AdmissionPolicy;
use cdk::nuts::{CurrencyUnit, State, SwapRequest};
use std::collections::HashMap;

impl<T: Transport> OperationGate<T> {
    pub(crate) fn arm_split_planning(&self, id: Uuid) -> Result<OperationLease> {
        let treasury = self
            .treasury
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("treasury disabled"))?;
        let policy = self
            .split_policy
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("splitting disabled"))?;
        ensure!(
            policy.enabled && self.database.is_some() && self.key_policy.is_some(),
            "split planning disabled or missing dependencies"
        );
        let mut active = self
            .active
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet coordinator unavailable"))?;
        ensure!(active.is_none(), "another wallet operation is active");
        let issued = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .treasury_instruction(&treasury.binding, id)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        treasury.binding.validate_new_invoice(&issued, now)?;
        *active = Some(Operation {
            id,
            authority: Authority::SplitPlanning,
        });
        Ok(OperationLease {
            active: self.active.clone(),
            id,
        })
    }

    pub fn with_split_policy(mut self, policy: Arc<AdmissionPolicy>) -> Result<Self> {
        policy.validate()?;
        self.split_policy = Some(policy);
        Ok(self)
    }
    pub fn arm_split(&self, id: Uuid) -> Result<OperationLease> {
        let treasury = self
            .treasury
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("treasury disabled"))?;
        let policy = self
            .split_policy
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("splitting disabled"))?;
        ensure!(
            self.database.is_some() && self.key_policy.is_some(),
            "split dependencies missing"
        );
        let mut active = self
            .active
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet coordinator unavailable"))?;
        ensure!(active.is_none(), "another wallet operation is active");
        let mut journal = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
        journal.unsubmitted_treasury_split(&treasury.binding, id, policy.limits)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        journal.authorize_treasury_split(&treasury.binding, id, policy, i64::try_from(now)?)?;
        journal.claim_treasury_split_execution(&treasury.binding, id, policy, now)?;
        *active = Some(Operation {
            id,
            authority: Authority::Split,
        });
        Ok(OperationLease {
            active: self.active.clone(),
            id,
        })
    }
    pub fn arm_split_recovery(&self, id: Uuid) -> Result<OperationLease> {
        let treasury = self
            .treasury
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("treasury scope missing"))?;
        ensure!(
            self.database.is_some() && self.key_policy.is_some(),
            "split recovery dependencies missing"
        );
        let mut active = self
            .active
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet coordinator unavailable"))?;
        ensure!(active.is_none(), "another wallet operation is active");
        let journal = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
        let derivation = journal.treasury_split_derivation(&treasury.binding, id)?;
        ensure!(
            derivation.mint_url == treasury.mint.advertised_url.parse()?,
            "split recovery mint changed"
        );
        *active = Some(Operation {
            id,
            authority: Authority::SplitRecovery,
        });
        Ok(OperationLease {
            active: self.active.clone(),
            id,
        })
    }
}
impl<T: Transport + 'static> OperationGate<T> {
    pub(super) async fn submit_split<P: Serialize + Send + Sync, R: DeserializeOwned>(
        &self,
        url: Url,
        payload: &P,
        bytes: &[u8],
    ) -> Result<R, HttpError> {
        let operation = self.active_operation()?;
        if operation.authority != Authority::Split {
            return Err(refused());
        }
        let id = operation.id;
        let treasury = self.treasury.as_ref().ok_or_else(refused)?;
        let policy = self.split_policy.as_ref().ok_or_else(refused)?;
        let database = self.database.as_ref().ok_or_else(refused)?;
        let (mint_id, keys) = self.key_policy.as_ref().ok_or_else(refused)?;
        if *mint_id != treasury.binding.mint_id {
            return Err(refused());
        }
        let planned = self
            .journal
            .lock()
            .map_err(|_| refused())?
            .unsubmitted_treasury_split(&treasury.binding, id, policy.limits)
            .map_err(|_| refused())?;
        let request: SwapRequest = serde_json::from_slice(bytes).map_err(|_| refused())?;
        let sagas = database
            .get_incomplete_sagas()
            .await
            .map_err(|_| refused())?;
        if sagas.len() > 1024 {
            return Err(refused());
        }
        let mut matching = None;
        for saga in &sagas {
            if saga.mint_url.join("v1/swap").map_err(|_| refused())? != url {
                continue;
            }
            if let Ok(derivation) = crate::ownership::SwapDerivation::capture_split(
                saga,
                &request,
                planned.plan().fee_sats(),
            ) {
                if matching.is_some() {
                    return Err(refused());
                }
                matching = Some((saga, derivation));
            }
        }
        let (saga, derivation) = matching.ok_or_else(refused)?;
        let configured: cdk::mint_url::MintUrl = treasury
            .mint
            .advertised_url
            .parse()
            .map_err(|_| refused())?;
        if saga.mint_url != configured {
            return Err(refused());
        }
        let owned = database
            .get_reserved_proofs(&saga.id)
            .await
            .map_err(|_| refused())?;
        if owned.len() != request.inputs().len()
            || owned
                .iter()
                .any(|p| p.state != State::Reserved || p.used_by_operation != Some(saga.id))
        {
            return Err(refused());
        }
        let ys = request
            .inputs()
            .iter()
            .map(|p| p.y())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| refused())?;
        let fresh: cdk::nuts::CheckStateResponse = self
            .inner
            .http_post(
                configured.join("v1/checkstate").map_err(|_| refused())?,
                None,
                &cdk::nuts::CheckStateRequest { ys },
            )
            .await?;
        crate::melt_derivation::verify_owned_inputs(
            request.inputs(),
            mint_id,
            &configured,
            &owned,
            keys,
            &fresh,
        )
        .map_err(|_| refused())?;
        let stored = database
            .get_keys(&derivation.keyset_id)
            .await
            .map_err(|_| refused())?
            .ok_or_else(refused)?;
        for output in &derivation.outputs {
            let pinned = keys
                .key(mint_id, output.keyset_id, u64::from(output.amount))
                .map_err(|_| refused())?;
            if stored.amount_key(output.amount) != Some(pinned) {
                return Err(refused());
            }
        }
        let keysets = database
            .get_mint_keysets(configured.clone())
            .await
            .map_err(|_| refused())?
            .ok_or_else(refused)?;
        let mut counts = HashMap::new();
        let mut fees = HashMap::new();
        for proof in request.inputs() {
            let matches = keysets
                .iter()
                .filter(|k| k.id == proof.keyset_id)
                .collect::<Vec<_>>();
            if matches.len() != 1 || matches[0].unit != CurrencyUnit::Sat {
                return Err(refused());
            }
            *counts.entry(proof.keyset_id).or_insert(0u64) += 1;
            fees.insert(proof.keyset_id, matches[0].input_fee_ppk);
        }
        let split_fee = u64::from(
            cdk::fees::calculate_fee(&counts, &fees)
                .map_err(|_| refused())?
                .total,
        );
        if split_fee != planned.plan().fee_sats() {
            return Err(refused());
        }
        let output_metadata = keysets
            .iter()
            .filter(|k| k.id == derivation.keyset_id)
            .collect::<Vec<_>>();
        if output_metadata.len() != 1
            || output_metadata[0].unit != CurrencyUnit::Sat
            || !output_metadata[0].active
        {
            return Err(refused());
        }
        let melt_count = u64::try_from(
            planned
                .plan()
                .outputs()
                .iter()
                .filter(|o| o.purpose == crate::treasury_split::Purpose::Treasury)
                .count(),
        )
        .map_err(|_| refused())?;
        let melt_fee = u64::from(
            cdk::fees::calculate_fee(
                &HashMap::from([(derivation.keyset_id, melt_count)]),
                &HashMap::from([(derivation.keyset_id, output_metadata[0].input_fee_ppk)]),
            )
            .map_err(|_| refused())?
            .total,
        );
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| refused())?
            .as_secs();
        {
            let mut journal = self.journal.lock().map_err(|_| refused())?;
            journal
                .freeze_treasury_split_request(
                    &treasury.binding,
                    &treasury.mint,
                    id,
                    bytes,
                    saga,
                    &treasury.seed,
                    policy.limits,
                    now,
                )
                .map_err(|_| refused())?;
            journal
                .claim_treasury_split_submission(
                    &treasury.binding,
                    id,
                    &url,
                    bytes,
                    melt_fee,
                    policy,
                    i64::try_from(now).map_err(|_| refused())?,
                )
                .map_err(|_| refused())?;
        }
        let response: serde_json::Value = self.inner.http_post(url, None, payload).await?;
        let reply = serialize_body(&response, self.limit)?;
        self.journal
            .lock()
            .map_err(|_| refused())?
            .record_treasury_split_reply(id, &reply)
            .map_err(|_| refused())?;
        serde_json::from_value(response).map_err(|_| refused())
    }
}
