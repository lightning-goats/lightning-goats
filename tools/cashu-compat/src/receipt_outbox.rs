//! Durable acquired-receipt retries. This worker has no mint or treasury authority.
use crate::{receipt_delivery::Client, service::Runtime};
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::{path::PathBuf, time::Duration};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub collector_handoff: bool,
    pub socket: PathBuf,
    pub core_epoch: Uuid,
    #[serde(default)]
    pub maximum_attempts: u32,
    pub deadline_seconds: u64,
}
impl Config {
    pub fn validate(&self, core_uid: u32) -> Result<()> {
        Client::new(
            self.socket.clone(),
            core_uid,
            self.core_epoch,
            Duration::from_secs(self.deadline_seconds),
        )?;
        ensure!(
            self.maximum_attempts <= 10_000 && (!self.enabled || self.maximum_attempts > 0),
            "receipt delivery requires bounded explicit attempts"
        );
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Tick {
    Idle,
    Delivered,
    RetryRetained,
}
pub struct Worker<'a> {
    runtime: &'a Runtime,
    client: Option<Client>,
}
impl<'a> Worker<'a> {
    pub fn new(runtime: &'a Runtime) -> Result<Self> {
        let client = match &runtime.config.receipt_delivery {
            Some(config) if config.enabled => {
                config.validate(runtime.config.core_uid)?;
                Some(Client::new(
                    config.socket.clone(),
                    runtime.config.core_uid,
                    config.core_epoch,
                    Duration::from_secs(config.deadline_seconds),
                )?)
            }
            _ => None,
        };
        Ok(Self { runtime, client })
    }
    pub async fn tick(&self, now: i64) -> Result<Tick> {
        let (Some(client), Some(config)) = (&self.client, &self.runtime.config.receipt_delivery)
        else {
            return Ok(Tick::Idle);
        };
        let id = self
            .runtime
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .claim_receipt_delivery(now, config.maximum_attempts, config.deadline_seconds)?;
        let Some(id) = id else {
            return Ok(Tick::Idle);
        };
        Ok(
            if client
                .deliver(
                    &self.runtime.journal,
                    self.runtime.config.wallet_instance,
                    id,
                )
                .await
                .is_ok()
            {
                Tick::Delivered
            } else {
                Tick::RetryRetained
            },
        )
    }
    pub async fn run(&self) -> Result<()> {
        loop {
            let now = i64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs(),
            )?;
            self.tick(now).await?;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delivery_is_off_without_explicit_toggle_and_budget() -> Result<()> {
        let mut config: Config = serde_json::from_value(
            serde_json::json!({"socket":"/run/core/receipt.sock","core_epoch":Uuid::new_v4(),"deadline_seconds":5}),
        )?;
        config.validate(12345)?;
        assert!(!config.enabled);
        assert_eq!(config.maximum_attempts, 0);
        config.enabled = true;
        assert!(config.validate(12345).is_err());
        config.maximum_attempts = 2;
        config.validate(12345)?;
        config.maximum_attempts = 10_001;
        assert!(config.validate(12345).is_err());
        config.maximum_attempts = 2;
        config.core_epoch = Uuid::nil();
        assert!(config.validate(12345).is_err());
        Ok(())
    }
}
