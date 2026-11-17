//! One serialized acquisition coordinator for ordinary Cashu and NIP-61 inputs.
//! No core-ledger, Strike, Nostr signer or physical gateway authority lives here.
use crate::{
    intake::{PinnedKeys, ValidatedContribution},
    journal::{Admission, HoldReason, Journal, Receipt},
    mint_policy::MintPolicy,
    operation_gate::OperationGate,
    ownership::verify_owned_outputs,
    transport::MintTransport,
    writer_lock::WriterLock,
};
use anyhow::{Result, ensure};
use cdk::{
    cdk_database::{Error as DatabaseError, WalletDatabase},
    mint_url::MintUrl,
    nuts::{CheckStateRequest, SecretKey},
    wallet::{ReceiveOptions, Wallet},
};
use cdk_http_client::Transport;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use uuid::Uuid;

#[derive(Clone, Copy, Default)]
pub struct Permissions {
    pub admission: bool,
    pub acquisition: bool,
    pub receipt_export: bool,
    pub original_swap_replay: bool,
}

pub struct Settings {
    pub timeout: Duration,
    pub message_bytes: usize,
    pub maximum_receive_fee_sats: u64,
}

pub enum Progress {
    Pending(Uuid),
    Held(Uuid),
    Acquired(Uuid),
    Receipt(Box<Receipt>),
}

/// Seed ownership/zeroization belongs to protected configuration; this type only borrows it.
pub struct Coordinator<'a, T: Transport> {
    wallet: Wallet,
    database: Arc<dyn WalletDatabase<DatabaseError> + Send + Sync>,
    gate: OperationGate<T>,
    journal: Arc<Mutex<Journal>>,
    keys: Arc<PinnedKeys>,
    seed: &'a [u8; 64],
    mint_id: String,
    mint_url: MintUrl,
    permissions: Permissions,
    maximum_receive_fee_sats: u64,
}

impl<'a> Coordinator<'a, MintTransport> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        policy: &MintPolicy,
        database: Arc<dyn WalletDatabase<DatabaseError> + Send + Sync>,
        seed: &'a [u8; 64],
        journal: Arc<Mutex<Journal>>,
        writer: Arc<WriterLock>,
        keys: Arc<PinnedKeys>,
        settings: Settings,
        permissions: Permissions,
    ) -> Result<Self> {
        ensure!(
            settings.maximum_receive_fee_sats <= i64::MAX as u64,
            "invalid receive fee authority"
        );
        let (wallet, gate) = crate::wallet::build_journaled_wallet(
            policy,
            database.clone(),
            *seed,
            settings.timeout,
            settings.message_bytes,
            journal.clone(),
            writer,
            keys.clone(),
        )?;
        Ok(Self {
            wallet,
            database,
            gate,
            journal,
            keys,
            seed,
            mint_id: policy.id.clone(),
            mint_url: policy.advertised_url.parse()?,
            permissions,
            maximum_receive_fee_sats: settings.maximum_receive_fee_sats,
        })
    }
}
impl<T: Transport + 'static> Coordinator<'_, T> {
    /// Admission is durable independently of worker availability. It grants no credit.
    pub fn admit(&self, contribution: &ValidatedContribution) -> Result<Admission> {
        ensure!(self.permissions.admission, "Cashu admission disabled");
        let intake = contribution.intake();
        ensure!(
            intake.mint_id == self.mint_id,
            "contribution belongs to another mint"
        );
        self.journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .admit(&intake)
    }

    pub fn set_permissions(&mut self, permissions: Permissions) {
        self.permissions = permissions;
    }

    fn ready(&self, id: Uuid) -> Result<Option<Progress>> {
        let mut journal = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
        ensure!(
            journal.mint_id(id)? == self.mint_id,
            "operation belongs to another mint"
        );
        match journal.state(id)?.as_str() {
            "held_for_reconciliation" => Ok(Some(Progress::Held(id))),
            "acquired" | "credit_export_pending" | "credited" => {
                if self.permissions.receipt_export {
                    Ok(Some(Progress::Receipt(Box::new(
                        journal.export_receipt(id)?,
                    ))))
                } else {
                    Ok(Some(Progress::Acquired(id)))
                }
            }
            _ => Ok(None),
        }
    }

    /// Retry of an already submitted acquisition never starts a new swap.
    pub async fn acquire_new(&mut self, id: Uuid, receiving_key: &SecretKey) -> Result<Progress> {
        if let Some(progress) = self.ready(id)? {
            return Ok(progress);
        }
        ensure!(self.permissions.acquisition, "Cashu acquisition disabled");
        let state = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .state(id)?;
        if state != "validated" {
            return Ok(Progress::Pending(id));
        }
        // Existing CDK work must be reconciled, even if a crash preceded the app hook.
        let sagas = self.database.get_incomplete_sagas().await?;
        if sagas.iter().any(|saga| saga.mint_url == self.mint_url) {
            return Ok(Progress::Pending(id));
        }
        let proofs = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .admitted_proofs(id)?;
        let _lease = self
            .gate
            .arm_new_acquisition(id, self.maximum_receive_fee_sats)
            .await?;
        let options = ReceiveOptions {
            p2pk_signing_keys: vec![receiving_key.clone()],
            ..Default::default()
        };
        // Any error retains the original journal. A timeout is never failure evidence.
        if self
            .wallet
            .receive_proofs(proofs, options, None, None)
            .await
            .is_err()
        {
            return Ok(Progress::Pending(id));
        }
        self.reconcile_owned(id).await
    }

    /// Read-only mint checks can reconcile outputs already persisted by CDK,
    /// including after a response/acknowledgment was lost. No replay occurs here.
    pub async fn reconcile_owned(&self, id: Uuid) -> Result<Progress> {
        self.check_owned(id, false).await
    }

    /// Explicit operator recheck of a hold raised while verifying original outputs.
    /// Returning acquired here never exports a receipt, even if the worker can.
    pub(crate) async fn recheck_held_ownership(&self, id: Uuid) -> Result<Progress> {
        self.check_owned(id, true).await
    }

    async fn check_owned(&self, id: Uuid, recheck_hold: bool) -> Result<Progress> {
        if recheck_hold {
            let journal = self
                .journal
                .lock()
                .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
            ensure!(
                journal.mint_id(id)? == self.mint_id
                    && journal.has_recheckable_ownership_hold(id)?,
                "operation has no eligible ownership hold for this mint"
            );
            match journal.state(id)?.as_str() {
                "held_for_reconciliation" => {}
                "acquired" => return Ok(Progress::Acquired(id)),
                _ => anyhow::bail!("operation is no longer available for hold recheck"),
            }
        } else if let Some(progress) = self.ready(id)? {
            return Ok(progress);
        }
        let unresolved = if recheck_hold {
            Progress::Held(id)
        } else {
            Progress::Pending(id)
        };
        let derivation = {
            let journal = self
                .journal
                .lock()
                .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
            match journal.derivation(id) {
                Ok(binding) => binding,
                Err(_) => return Ok(unresolved),
            }
        };
        ensure!(
            derivation.mint_url == self.mint_url,
            "recovery mint binding changed"
        );
        let ys = derivation.output_ys(self.seed)?;
        let owned = self.database.get_proofs_by_ys(ys.clone()).await?;
        if owned.len() != ys.len() {
            return Ok(unresolved);
        }
        let states = match self
            .wallet
            .mint_connector()
            .post_check_state(CheckStateRequest { ys })
            .await
        {
            Ok(states) => states,
            Err(_) => return Ok(unresolved),
        };
        {
            let mut journal = self
                .journal
                .lock()
                .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
            match verify_owned_outputs(
                id,
                &journal,
                &derivation,
                self.seed,
                &self.mint_id,
                &self.keys,
                &owned,
                &states,
            ) {
                Ok(acquired) => acquired.commit(&mut journal, recheck_hold)?,
                Err(_) => {
                    journal.hold(id, HoldReason::MissingOwnedOutputs)?;
                    return Ok(Progress::Held(id));
                }
            }
        }
        if recheck_hold {
            return Ok(Progress::Acquired(id));
        }
        Ok(self.ready(id)?.unwrap_or(Progress::Acquired(id)))
    }

    /// Original replay is separately authorized, always through the exact-byte gate.
    pub async fn recover_original(&mut self, id: Uuid) -> Result<Progress> {
        let progress = self.reconcile_owned(id).await?;
        if !matches!(progress, Progress::Pending(_)) {
            return Ok(progress);
        }
        ensure!(
            self.permissions.original_swap_replay,
            "original swap replay disabled"
        );
        let binding = {
            self.journal
                .lock()
                .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
                .derivation(id)?
        };
        let sagas = self.database.get_incomplete_sagas().await?;
        let scoped = sagas
            .iter()
            .filter(|saga| saga.mint_url == self.mint_url)
            .collect::<Vec<_>>();
        // CDK's public recovery API is wallet-wide: never let it process a second
        // financial operation under the current acquisition's authority.
        if scoped.len() != 1 || scoped[0].id != binding.cdk_operation {
            return Ok(Progress::Pending(id));
        }
        let _lease = self.gate.arm(id, true, self.maximum_receive_fee_sats)?;
        let _report = self.wallet.recover_incomplete_sagas().await;
        // Even a Recovered report must pass independently verified ownership.
        self.reconcile_owned(id).await
    }
}

#[cfg(test)]
mod tests;
