//! Serialized bounded acquisition/recovery work, with durable retry admission.
use crate::{
    acquisition::{Coordinator, Permissions, Settings},
    service::Runtime,
    transport::MintTransport,
};
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::{collections::BTreeMap, sync::Arc, time::Duration};

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Execution {
    #[serde(default)]
    pub acquire: bool,
    #[serde(default)]
    pub reconcile: bool,
    #[serde(default)]
    pub original_swap_replay: bool,
    #[serde(default)]
    pub maximum_attempts: u32,
    #[serde(default)]
    pub maximum_receive_fee_sats: u64,
}
impl Execution {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.maximum_attempts <= 10_000 && self.maximum_receive_fee_sats <= i64::MAX as u64,
            "invalid acquisition execution bounds"
        );
        ensure!(
            !self.original_swap_replay || self.reconcile,
            "original replay requires reconciliation permission"
        );
        ensure!(
            !(self.acquire || self.reconcile) || self.maximum_attempts > 0,
            "acquisition execution requires an explicit attempt budget"
        );
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Tick {
    Idle,
    Progress,
    RetryRetained,
}

pub struct Worker<'a> {
    runtime: &'a Runtime,
    coordinators: BTreeMap<String, Coordinator<'a, MintTransport>>,
}
impl<'a> Worker<'a> {
    pub async fn run(&mut self) -> Result<()> {
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

    pub fn new(runtime: &'a Runtime) -> Result<Self> {
        let config = &runtime.config;
        config.execution.validate()?;
        let mut coordinators = BTreeMap::new();
        let keys = Arc::new(runtime.policy.keys.clone());
        for policy in &config.mints {
            // Retained configured mints may recover after new receiving is disabled.
            if !policy.bolt11_exit_qualified {
                continue;
            }
            let coordinator = Coordinator::new(
                policy,
                runtime
                    .mint_databases
                    .get(&policy.id)
                    .ok_or_else(|| anyhow::anyhow!("configured mint namespace missing"))?
                    .clone(),
                &runtime.keys.seed,
                runtime.journal.clone(),
                runtime.writer.clone(),
                keys.clone(),
                Settings {
                    timeout: Duration::from_secs(config.deadline_seconds),
                    message_bytes: config.body_bytes,
                    maximum_receive_fee_sats: config.execution.maximum_receive_fee_sats,
                },
                Permissions {
                    admission: false,
                    acquisition: config.execution.acquire,
                    receipt_export: false,
                    original_swap_replay: config.execution.original_swap_replay,
                },
            )?;
            coordinators.insert(policy.id.clone(), coordinator);
        }
        Ok(Self {
            runtime,
            coordinators,
        })
    }

    /// At most one durable attempt. A local recovery fence leaves the attempt
    /// budget untouched and allows a candidate from a different mint to run.
    pub async fn tick(&mut self, now: i64) -> Result<Tick> {
        let config = &self.runtime.config;
        let started = tokio::time::Instant::now();
        let configured: Vec<_> = self.coordinators.keys().map(String::as_str).collect();
        let candidates = self
            .runtime
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .ready_acquisition_work(
                now,
                config.execution.acquire,
                config.execution.reconcile,
                config.execution.maximum_attempts,
                Some(&configured),
            )?;
        for work in candidates {
            let coordinator = self
                .coordinators
                .get_mut(&work.mint_id)
                .ok_or_else(|| anyhow::anyhow!("configured acquisition coordinator missing"))?;
            let deadline =
                tokio::time::Instant::now() + Duration::from_secs(config.deadline_seconds);
            let prepared = if work.state == "validated" {
                let Ok(Ok(Some(prepared))) =
                    tokio::time::timeout_at(deadline, coordinator.prepare_new_acquisition(work.id))
                        .await
                else {
                    continue;
                };
                Some(prepared)
            } else {
                None
            };
            let attempt_at = now
                .checked_add(i64::try_from(started.elapsed().as_secs())?)
                .ok_or_else(|| anyhow::anyhow!("acquisition schedule overflow"))?;
            if tokio::time::Instant::now() >= deadline
                || !self
                    .runtime
                    .journal
                    .lock()
                    .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
                    .charge_acquisition_work(&work, attempt_at, config.deadline_seconds)?
            {
                continue;
            }
            let result = tokio::time::timeout_at(deadline, async {
                if let Some(prepared) = prepared {
                    coordinator
                        .submit_prepared_acquisition(prepared, |lock| {
                            self.runtime.keys.signing_key(lock)
                        })
                        .await
                } else if config.execution.original_swap_replay {
                    coordinator.recover_original(work.id).await
                } else {
                    coordinator.reconcile_owned(work.id).await
                }
            })
            .await;
            return Ok(match result {
                Ok(Ok(
                    crate::acquisition::Progress::Acquired(_)
                    | crate::acquisition::Progress::Receipt(_),
                )) => Tick::Progress,
                _ => Tick::RetryRetained,
            });
        }
        Ok(Tick::Idle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn execution_is_off_without_explicit_permissions_and_retry_budget() -> Result<()> {
        let execution: Execution = serde_json::from_str("{}")?;
        execution.validate()?;
        assert!(!execution.acquire && !execution.reconcile && !execution.original_swap_replay);
        assert_eq!(execution.maximum_attempts, 0);
        assert_eq!(execution.maximum_receive_fee_sats, 0);
        for json in [
            r#"{"acquire":true}"#,
            r#"{"reconcile":true}"#,
            r#"{"original_swap_replay":true,"maximum_attempts":1}"#,
            r#"{"maximum_attempts":10001}"#,
            r#"{"maximum_receive_fee_sats":18446744073709551615}"#,
        ] {
            assert!(serde_json::from_str::<Execution>(json)?.validate().is_err());
        }
        serde_json::from_str::<Execution>(
            r#"{"acquire":true,"reconcile":true,"maximum_attempts":2}"#,
        )?
        .validate()?;
        assert!(serde_json::from_str::<Execution>(r#"{"withdraw":true}"#).is_err());
        Ok(())
    }
}
