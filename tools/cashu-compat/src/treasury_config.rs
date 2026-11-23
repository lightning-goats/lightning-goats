//! Optional private admission/status endpoint. Enabling it grants no melt execution.
use crate::{service_config, treasury::TreasuryBinding};
use anyhow::{Result, ensure};
use lightning_goats_cashu_receipt::treasury::Destination;
use serde::Deserialize;
use std::{collections::BTreeMap, path::PathBuf};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub accept_new: bool,
    pub socket: PathBuf,
    pub core_epoch: Uuid,
    pub destination: Destination,
    /// Mint ID -> immutable treasury policy version, independent of receive policy.
    pub policies: BTreeMap<String, String>,
}
impl Config {
    pub fn bindings(
        &self,
        wallet: &service_config::Config,
    ) -> Result<BTreeMap<String, TreasuryBinding>> {
        crate::private_ipc::socket_path(&self.socket)?;
        self.destination.validate()?;
        ensure!(
            !self.core_epoch.is_nil() && (!self.accept_new || self.enabled),
            "invalid treasury admission configuration"
        );
        ensure!(
            self.socket != wallet.intake_socket
                && !self.socket.starts_with(&wallet.state_directory)
                && wallet
                    .publication_signing
                    .as_ref()
                    .is_none_or(|c| c.socket != self.socket)
                && wallet
                    .receipt_delivery
                    .as_ref()
                    .is_none_or(|c| c.socket != self.socket && c.core_epoch == self.core_epoch),
            "treasury endpoint must have a separate socket and matching core epoch"
        );
        ensure!(
            !self.policies.is_empty() && self.policies.len() <= 16,
            "invalid treasury policy inventory"
        );
        let mut bindings = BTreeMap::new();
        for (mint_id, version) in &self.policies {
            ensure!(
                !mint_id.is_empty()
                    && mint_id.len() <= 128
                    && !mint_id.chars().any(char::is_control)
                    && !version.is_empty()
                    && version.len() <= 128
                    && !version.chars().any(char::is_control),
                "invalid treasury policy identity"
            );
            let mint = wallet
                .mints
                .iter()
                .find(|mint| mint.id == *mint_id)
                .ok_or_else(|| anyhow::anyhow!("treasury mint missing from protected inventory"))?;
            ensure!(
                !self.accept_new || mint.bolt11_exit_qualified,
                "new treasury admission requires qualified mint exit"
            );
            bindings.insert(
                mint_id.clone(),
                TreasuryBinding {
                    core_epoch: self.core_epoch,
                    wallet_instance: wallet.wallet_instance,
                    wallet_epoch: wallet.wallet_epoch,
                    destination: self.destination.clone(),
                    mint_id: mint_id.clone(),
                    policy_version: version.clone(),
                },
            );
        }
        Ok(bindings)
    }
}
