//! Optional presentation task set. The only credential is a pinned NIP-46
//! authentication client, never a project signing key or Strike credential.
use super::{calendar, store::Store};
use crate::{core_bridge::unix::Client, nostr::NakClient};
use anyhow::{Result, bail};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    pub start_after: i64,
    pub poll_millis: u64,
    pub relays: Vec<String>,
    #[serde(default)]
    pub signer: Option<Signer>,
}
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Signer {
    pub nak_path: PathBuf,
    pub bunker_pubkey: String,
    pub client_pubkey: String,
    pub bunker_relays: Vec<String>,
}
impl Config {
    pub fn validate(&self, owner: &str) -> Result<()> {
        crate::payout::invoice::receipt_relays(&self.relays)?;
        if self.start_after < 0 || !(500..=60000).contains(&self.poll_millis) {
            bail!("invalid member messaging activation/poll configuration");
        }
        if let Some(signer) = &self.signer {
            crate::nostr_event::validate_public_key(&signer.client_pubkey)?;
            if signer.bunker_pubkey != owner
                || signer.client_pubkey == owner
                || !signer.nak_path.is_absolute()
            {
                bail!(
                    "member signer must use a distinct pinned NIP-46 client and exact project identity"
                );
            }
            crate::payout::invoice::receipt_relays(&signer.bunker_relays)?;
        }
        Ok(())
    }
    pub(crate) async fn start(
        &self,
        store: Store,
        owner: &str,
        state: &Path,
        core_socket: &Path,
        core_uid: u32,
        tasks: &mut tokio::task::JoinSet<()>,
    ) -> Result<()> {
        self.validate(owner)?;
        if !self.enabled {
            return Ok(());
        }
        store.initialize_delivery(owner, self.start_after).await?;
        let nak = if let Some(signer) = &self.signer {
            crate::private_ipc::protected_config(&signer.nak_path)?;
            let key = zeroize::Zeroizing::new(
                crate::secrets::read_systemd_credential("nostr-client-key").await?,
            );
            crate::nostr::validate_client_credential(owner, &signer.client_pubkey, &key)?;
            Some(NakClient::new(
                &crate::config::NostrConfig {
                    nak_path: signer.nak_path.clone(),
                    nak_config_path: state.join("nak"),
                    bunker_pubkey: owner.into(),
                    relays: vec![],
                    bunker_relays: Some(signer.bunker_relays.clone()),
                },
                key.to_string(),
            )?)
        } else {
            None
        };
        let millis = self.poll_millis;
        let generator_store = store.clone();
        let generator_owner = owner.to_owned();
        let relays = self.relays.clone();
        tasks.spawn(async move {
            let mut ticks = tokio::time::interval(Duration::from_millis(millis));
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticks.tick().await;
                let result = async {
                    generator_store
                        .queue_next_delivery(&generator_owner, &relays, calendar::now()?)
                        .await?;
                    generator_store
                        .resolve_legacy_delivery(&generator_owner)
                        .await?;
                    anyhow::Ok(())
                }
                .await;
                if result.is_err() {
                    tracing::warn!("member message generation held; frozen source cursor retained");
                }
            }
        });
        let overlay_store = store.clone();
        let overlay_owner = owner.to_owned();
        let overlay_core = Client::new(core_socket.to_path_buf(), core_uid)?;
        tasks.spawn(async move {
            let mut ticks = tokio::time::interval(Duration::from_millis(millis));
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticks.tick().await;
                let result = async {
                    if let Some(message) =
                        overlay_store.pending_member_overlay(&overlay_owner).await?
                    {
                        let seq = overlay_core.member_overlay(&message).await?;
                        overlay_store
                            .acknowledge_member_overlay(&message, seq)
                            .await?;
                    }
                    anyhow::Ok(())
                }
                .await;
                if result.is_err() {
                    tracing::warn!("member overlay delivery held; original message retained");
                }
            }
        });
        if let Some(nak) = nak {
            let worker = super::delivery_worker::Worker {
                store,
                core: Client::new(core_socket.to_path_buf(), core_uid)?,
                transport: nak,
                owner: owner.into(),
            };
            tasks.spawn(async move {
                let mut ticks = tokio::time::interval(Duration::from_millis(millis));
                ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    ticks.tick().await;
                    if worker.tick().await.is_err() {
                        tracing::warn!("member Nostr delivery held; exact signed events retained");
                    }
                }
            });
        }
        Ok(())
    }
}
