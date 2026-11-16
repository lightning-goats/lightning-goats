//! Core orchestration of retained intents; provider recovery independently verifies deposits.
use super::{
    receipt_authority::ReceiptPolicy,
    treasury::Policy,
    treasury_client::{Client, Response},
};
use crate::{ledger::LedgerStore, strike::StrikeRuntime};
use anyhow::{Result, ensure};
use lightning_goats_cashu_receipt::treasury_rpc::{Command, Request};
use serde::Deserialize;
use std::{future::Future, sync::Arc, time::Duration};

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Controls {
    #[serde(default)]
    pub sweep: super::treasury_sweep::Policy,
    #[serde(default)]
    pub plan: bool,
    #[serde(default)]
    pub issue: bool,
    #[serde(default)]
    pub deliver: bool,
    #[serde(default)]
    pub inspect: bool,
    #[serde(default)]
    pub maximum_attempts: u32,
}
impl Controls {
    pub fn validate(&self) -> Result<()> {
        self.sweep.validate_shape()?;
        ensure!(
            self.maximum_attempts <= 10_000
                && (!(self.plan
                    || self.sweep.enabled
                    || self.issue
                    || self.deliver
                    || self.inspect)
                    || self.maximum_attempts > 0),
            "invalid core treasury attempt limit"
        );
        ensure!(
            !(self.plan || self.sweep.enabled || self.issue || self.deliver) || self.inspect,
            "core treasury execution requires inspection"
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    Issue,
    Wallet,
}
pub(crate) struct Work {
    pub id: uuid::Uuid,
    pub stage: Stage,
}

pub trait Wallet: Send + Sync {
    fn exchange(&self, request: &Request) -> impl Future<Output = Result<Response>> + Send;
}
impl Wallet for Client {
    async fn exchange(&self, request: &Request) -> Result<Response> {
        self.send(request).await
    }
}
pub struct Worker<W: Wallet = Client> {
    ledger: LedgerStore,
    strike: StrikeRuntime,
    registration: Arc<ReceiptPolicy>,
    policy: Arc<Policy>,
    wallet: W,
    controls: Controls,
    deadline: Duration,
}
impl<W: Wallet> Worker<W> {
    pub fn new(
        ledger: LedgerStore,
        strike: StrikeRuntime,
        registration: Arc<ReceiptPolicy>,
        policy: Arc<Policy>,
        wallet: W,
        controls: Controls,
        deadline: Duration,
    ) -> Result<Self> {
        controls.validate()?;
        controls.sweep.validate(&policy)?;
        ensure!(
            deadline.as_secs() >= 1 && deadline <= Duration::from_secs(60),
            "invalid core treasury deadline"
        );
        Ok(Self {
            ledger,
            strike,
            registration,
            policy,
            wallet,
            controls,
            deadline,
        })
    }
    pub async fn tick(&self, now: i64) -> Result<Tick> {
        let controls = Controls {
            issue: self.controls.issue && self.policy.enabled,
            ..self.controls.clone()
        };
        let Some(work) = self
            .ledger
            .claim_cashu_treasury_work(&self.registration, &controls, now, self.deadline.as_secs())
            .await?
        else {
            if self.controls.plan && self.policy.enabled {
                match tokio::time::timeout(
                    self.deadline,
                    self.ledger
                        .plan_cashu_feed_treasury(&self.registration, &self.policy),
                )
                .await
                {
                    Ok(Ok(Some(_))) => return Ok(Tick::Progress),
                    Ok(Ok(None)) => {}
                    _ => return Ok(Tick::RetryRetained),
                }
            }
            if self.controls.sweep.enabled && self.policy.enabled {
                return Ok(
                    match tokio::time::timeout(
                        self.deadline,
                        self.ledger.plan_cashu_sweep(
                            &self.registration,
                            &self.policy,
                            &self.controls.sweep,
                        ),
                    )
                    .await
                    {
                        Ok(Ok(Some(_))) => Tick::Progress,
                        Ok(Ok(None)) => Tick::Idle,
                        _ => Tick::RetryRetained,
                    },
                );
            }
            return Ok(Tick::Idle);
        };
        Ok(
            match tokio::time::timeout(self.deadline, self.advance(work)).await {
                Ok(Ok(true)) => Tick::Progress,
                _ => Tick::RetryRetained,
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
    async fn advance(&self, work: Work) -> Result<bool> {
        match work.stage {
            Stage::Issue => Ok(self
                .strike
                .create_cashu_treasury_invoice(
                    &self.ledger,
                    &self.registration,
                    &self.policy,
                    work.id,
                )
                .await?
                .is_some()),
            Stage::Wallet => {
                let issued = self
                    .ledger
                    .cashu_treasury_invoice(work.id)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("treasury invoice missing"))?;
                let command =
                    if self.controls.deliver && self.policy.check_new(&issued.intent).is_ok() {
                        // Paused spending must not prevent inspection after a lost ACK.
                        Command::Retain(Box::new(issued))
                    } else {
                        Command::Status(Box::new(issued.intent))
                    };
                let request = Request {
                    version: 1,
                    request_id: uuid::Uuid::new_v4(),
                    command,
                };
                let response = self.wallet.exchange(&request).await?;
                response.reply.validate_for(&request)?;
                ensure!(
                    response.reply.report.is_some() == response.report.is_some(),
                    "treasury authenticated report missing"
                );
                if let Some(report) = response.report {
                    ensure!(
                        report.intent_id() == work.id,
                        "treasury report identity changed"
                    );
                    self.ledger
                        .record_cashu_melt_result(&self.registration, &report)
                        .await?;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controls_default_off_and_require_inspection_and_bounded_attempts() -> Result<()> {
        let off: Controls = serde_json::from_str("{}")?;
        off.validate()?;
        assert!(!off.sweep.enabled && !off.plan && !off.issue && !off.deliver && !off.inspect);
        for invalid in [
            r#"{"sweep":{"enabled":true},"inspect":true,"maximum_attempts":1}"#,
            r#"{"sweep":{"enabled":true,"balance_trigger_sats":100},"maximum_attempts":1}"#,
            r#"{"plan":true,"maximum_attempts":1}"#,
            r#"{"issue":true,"maximum_attempts":1}"#,
            r#"{"deliver":true,"maximum_attempts":1}"#,
            r#"{"inspect":true}"#,
            r#"{"maximum_attempts":10001}"#,
        ] {
            assert!(
                serde_json::from_str::<Controls>(invalid)?
                    .validate()
                    .is_err()
            );
        }
        serde_json::from_str::<Controls>(r#"{"inspect":true,"maximum_attempts":1}"#)?.validate()?;
        assert!(serde_json::from_str::<Controls>(r#"{"spend":true}"#).is_err());
        Ok(())
    }
}
