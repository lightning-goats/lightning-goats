//! Original reserved split execution and independent custody reconciliation.
use super::*;
use crate::journal::treasury_split::AdmissionPolicy;
use cdk::nuts::{CheckStateRequest, CheckStateResponse};

impl<T: Transport + Clone + 'static> Executor<T> {
    /// Select and freeze a bounded covering inventory without spending. Exact
    /// retained plans replay offline; metadata changes never select new proofs.
    pub async fn reserve_split(
        &self,
        id: Uuid,
    ) -> Result<crate::journal::treasury_split::Reservation> {
        let policy = self
            .split_policy
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("split policy not configured"))?;
        if let Some(retained) = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .retained_treasury_split(&self.policy.binding, id, policy.limits)?
        {
            return Ok(retained);
        }
        let _lease = self.gate.arm_split_planning(id)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let candidates = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .treasury_split_candidates(&self.policy.binding, id, now)?;
        ensure!(!candidates.is_empty(), "no available split candidates");
        let ys = candidates
            .iter()
            .map(|c| Ok(cdk::nuts::PublicKey::from_hex(&c.y)?))
            .collect::<Result<Vec<_>>>()?;
        let stored = self.database.get_proofs_by_ys(ys).await?;
        ensure!(
            stored.len() == candidates.len(),
            "split candidates missing from wallet"
        );
        let active = self
            .wallet
            .active_keyset()
            .await
            .map_err(|_| anyhow::anyhow!("active split keyset unavailable"))?;
        ensure!(
            active.unit == CurrencyUnit::Sat && active.active == Some(true),
            "invalid active split keyset"
        );
        let mut denominations = Vec::new();
        for (amount, key) in active.keys.iter() {
            ensure!(
                self.keys
                    .key(&self.policy.mint.id, active.id, u64::from(*amount))?
                    == *key,
                "split output key is not pinned"
            );
            denominations.push(u64::from(*amount));
        }
        denominations.sort();
        ensure!(
            !denominations.is_empty() && denominations.len() <= 64,
            "split denomination bound exceeded"
        );
        let mint_url: cdk::mint_url::MintUrl = self.policy.mint.advertised_url.parse()?;
        let keysets = self
            .database
            .get_mint_keysets(mint_url.clone())
            .await?
            .ok_or_else(|| anyhow::anyhow!("split input metadata missing"))?;
        ensure!(
            keysets.len() <= 1024,
            "split keyset metadata bound exceeded"
        );
        let mut fees = std::collections::BTreeMap::new();
        for candidate in &candidates {
            let rows = stored
                .iter()
                .filter(|p| p.y.to_hex() == candidate.y)
                .collect::<Vec<_>>();
            ensure!(rows.len() == 1, "ambiguous split wallet proof");
            let info = rows[0];
            ensure!(
                info.proof.y()? == info.y
                    && info.proof.amount == candidate.amount.into()
                    && info.mint_url == mint_url
                    && info.unit == CurrencyUnit::Sat
                    && info.state == State::Unspent
                    && info.used_by_operation.is_none(),
                "split candidate is unavailable or inconsistent"
            );
            ensure!(info.proof.dleq.is_some(), "split candidate lacks DLEQ");
            info.proof
                .verify_dleq(self.keys.key(
                    &self.policy.mint.id,
                    info.proof.keyset_id,
                    candidate.amount,
                )?)
                .map_err(|_| anyhow::anyhow!("invalid split candidate DLEQ"))?;
            let metadata = keysets
                .iter()
                .filter(|k| k.id == info.proof.keyset_id)
                .collect::<Vec<_>>();
            ensure!(
                metadata.len() == 1 && metadata[0].unit == CurrencyUnit::Sat,
                "ambiguous split input fee metadata"
            );
            fees.insert(candidate.y.clone(), metadata[0].input_fee_ppk);
        }
        self.journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .select_treasury_split(
                &self.policy.binding,
                id,
                Uuid::new_v4(),
                &fees,
                &denominations,
                policy.limits,
                now,
            )
    }

    /// Configure a separate whole-proof exposure budget. Rebuilding the local
    /// client starts no task and does not admit an operation or contact a mint.
    pub fn with_split_policy(mut self, policy: Arc<AdmissionPolicy>) -> Result<Self> {
        policy.validate()?;
        self.gate = self.gate.with_split_policy(policy.clone())?;
        self.wallet = WalletBuilder::new()
            .mint_url(self.policy.mint.advertised_url.parse()?)
            .unit(CurrencyUnit::Sat)
            .localstore(self.database.clone())
            .seed(self.policy.seed)
            .client(CustomHttpClient::with_transport(
                self.policy.mint.advertised_url.parse()?,
                self.gate.clone(),
                None,
            ))
            .use_http_subscription()
            .build()?;
        self.split_policy = Some(policy);
        Ok(self)
    }

    /// At most one CDK preparation and one POST for the retained reservation.
    /// Transport success is not custody settlement; reconcile_split is separate.
    pub async fn execute_split(&self, id: Uuid) -> Result<()> {
        let policy = self
            .split_policy
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("split execution is not configured"))?;
        let _lease = self.gate.arm_split(id)?;
        let mint_url: cdk::mint_url::MintUrl = self.policy.mint.advertised_url.parse()?;
        let sagas = self.database.get_incomplete_sagas().await?;
        ensure!(
            sagas.len() <= 1024 && !sagas.iter().any(|s| s.mint_url == mint_url),
            "existing mint operation requires recovery before splitting"
        );
        let reservation = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .unsubmitted_treasury_split(&self.policy.binding, id, policy.limits)?;
        let ys = reservation.input_ys()?;
        let stored = self.database.get_proofs_by_ys(ys.clone()).await?;
        let expected = ys
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        let actual = stored
            .iter()
            .map(|p| p.y)
            .collect::<std::collections::BTreeSet<_>>();
        ensure!(
            !ys.is_empty()
                && expected.len() == ys.len()
                && actual.len() == stored.len()
                && actual == expected,
            "reserved split proof inventory changed"
        );
        for proof in &stored {
            ensure!(
                proof.proof.y()? == proof.y
                    && proof.state == State::Unspent
                    && proof.used_by_operation.is_none()
                    && proof.unit == CurrencyUnit::Sat
                    && proof.mint_url == mint_url,
                "split proof is unavailable or belongs to another operation"
            );
        }
        let target = cdk::amount::SplitTarget::Values(
            reservation
                .plan()
                .outputs()
                .iter()
                .map(|o| o.amount.into())
                .collect(),
        );
        self.wallet
            .swap(
                None,
                target,
                stored.into_iter().map(|p| p.proof).collect(),
                None,
                false,
                false,
            )
            .await
            .map(|_| ())
            .map_err(|_| anyhow::anyhow!("split requires original-operation reconciliation"))
    }

    /// Recovery grants no quote, swap or melt execution authority. Independently
    /// verify the original owned outputs before committing custody accounting.
    pub async fn reconcile_split(&self, id: Uuid) -> Result<bool> {
        if self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .completed_treasury_split(&self.policy.binding, id)?
        {
            return Ok(true);
        }
        let _lease = self.gate.arm_split_recovery(id)?;
        let mint_url: cdk::mint_url::MintUrl = self.policy.mint.advertised_url.parse()?;
        let sagas = self.database.get_incomplete_sagas().await?;
        ensure!(sagas.len() <= 1024, "too many incomplete wallet operations");
        let scoped = sagas
            .iter()
            .filter(|s| s.mint_url == mint_url)
            .collect::<Vec<_>>();
        ensure!(
            scoped.len() <= 1,
            "unrelated mint operations prevent split recovery"
        );
        let (derivation, inputs) = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .treasury_split_recovery_context(&self.policy.binding, id, scoped.first().copied())?;
        if !scoped.is_empty() {
            // CDK recovery also cleans up orphaned quote reservations. Do not
            // let this split recovery mutate another operation's quote lease.
            let melt_quotes = self.database.get_melt_quotes().await?;
            let mint_quotes = self.database.get_mint_quotes().await?;
            ensure!(
                melt_quotes.len() <= 1024 && mint_quotes.len() <= 1024,
                "too many stored quotes for bounded split recovery"
            );
            ensure!(
                !melt_quotes.iter().any(|q| q.unit == CurrencyUnit::Sat
                    && q.mint_url.as_ref().is_none_or(|url| url == &mint_url)
                    && q.used_by_operation.is_some())
                    && !mint_quotes.iter().any(|q| q.unit == CurrencyUnit::Sat
                        && q.mint_url == mint_url
                        && q.used_by_operation.is_some()),
                "unrelated quote reservation prevents split recovery"
            );
            // The public CDK recovery API is wallet-wide. The checks above bind
            // its only scoped saga to this original request before any mutation.
            self.wallet
                .recover_incomplete_sagas()
                .await
                .map_err(|_| anyhow::anyhow!("original split recovery remains unresolved"))?;
        }
        let ys = derivation.output_ys(&self.policy.seed)?;
        let owned = self.database.get_proofs_by_ys(ys.clone()).await?;
        let fresh_inputs: CheckStateResponse = self
            .gate
            .http_post(
                mint_url.join("v1/checkstate")?,
                None,
                &CheckStateRequest { ys: inputs },
            )
            .await
            .map_err(|_| anyhow::anyhow!("split input state unavailable"))?;
        let fresh_outputs: CheckStateResponse = self
            .gate
            .http_post(
                mint_url.join("v1/checkstate")?,
                None,
                &CheckStateRequest { ys },
            )
            .await
            .map_err(|_| anyhow::anyhow!("split output state unavailable"))?;
        let mut journal = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
        let verified = journal.verify_treasury_split_outputs(
            &self.policy.binding,
            id,
            &self.policy.seed,
            &self.keys,
            &owned,
            &fresh_inputs,
            &fresh_outputs,
        )?;
        verified.commit(&mut journal, &self.policy.binding)?;
        Ok(true)
    }
}
