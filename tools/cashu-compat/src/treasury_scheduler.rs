//! Durable wallet-side treasury progression. Core settlement/feeding remains separate.
use crate::{
    intake::PinnedKeys,
    journal::{
        Journal,
        treasury_scheduling::{Action, Work},
    },
    operation_gate::TreasuryPolicy,
    service::Runtime,
    transport::MintTransport,
    treasury_execution::Executor,
    treasury_recovery::Recovery,
    writer_lock::WriterLock,
};
use anyhow::{Result, ensure};
use cdk::cdk_database::{Error as DatabaseError, WalletDatabase};
use cdk_http_client::Transport;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Execution {
    #[serde(default)]
    pub split: crate::journal::treasury_split::AdmissionPolicy,
    #[serde(default)]
    pub execute: bool,
    #[serde(default)]
    pub reconcile: bool,
    #[serde(default)]
    pub maximum_attempts: u32,
    #[serde(default)]
    pub maximum_input_sats: u64,
    #[serde(default)]
    pub maximum_fee_sats: u64,
    #[serde(default)]
    pub operations_per_24h: u64,
    #[serde(default)]
    pub unlimited_operations_per_24h: bool,
    #[serde(default)]
    pub input_sats_per_24h: u64,
    #[serde(default)]
    pub unlimited_input_sats_per_24h: bool,
    #[serde(default)]
    pub fee_sats_per_24h: Option<u64>,
    #[serde(default)]
    pub maximum_in_flight_sats: u64,
}
impl Execution {
    pub fn validate(&self) -> Result<()> {
        self.split.validate()?;
        ensure!(
            self.maximum_attempts <= 10_000
                && self.operations_per_24h <= 10_000
                && (!self.unlimited_operations_per_24h || self.operations_per_24h == 0)
                && (!(self.execute || self.reconcile) || self.maximum_attempts > 0),
            "invalid treasury attempt/admission budget"
        );
        ensure!(
            !self.execute || self.reconcile,
            "treasury execution requires reconciliation"
        );
        for value in [
            self.maximum_input_sats,
            self.input_sats_per_24h,
            self.maximum_in_flight_sats,
            self.maximum_fee_sats,
        ] {
            ensure!(value <= i64::MAX as u64, "invalid treasury amount bound");
        }
        ensure!(
            !self.execute
                || (self.maximum_input_sats > 0
                    && (self.operations_per_24h > 0 || self.unlimited_operations_per_24h)
                    && (self.input_sats_per_24h > 0 || self.unlimited_input_sats_per_24h)
                    && self.maximum_in_flight_sats > 0),
            "treasury execution requires explicit financial limits"
        );
        ensure!(
            (!self.unlimited_input_sats_per_24h || self.input_sats_per_24h == 0)
                && self
                    .fee_sats_per_24h
                    .is_none_or(|fee| fee <= i64::MAX as u64),
            "invalid treasury daily input or fee limits"
        );
        Ok(())
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum Tick {
    Idle,
    Progress,
    RetryRetained,
    UnconfiguredMint,
}
struct Context<T: Transport> {
    policy: Arc<TreasuryPolicy>,
    transport: T,
    executor: Option<Executor<T>>,
    database: Arc<dyn WalletDatabase<DatabaseError> + Send + Sync>,
}
pub struct Worker<T: Transport = MintTransport> {
    journal: Arc<Mutex<Journal>>,
    writer: Arc<WriterLock>,
    keys: Arc<PinnedKeys>,
    contexts: BTreeMap<String, Context<T>>,
    execution: Execution,
    deadline: Duration,
}
impl Worker<MintTransport> {
    pub fn new(runtime: &Runtime) -> Result<Self> {
        let config = &runtime.config;
        let mut contexts = Vec::new();
        if config.treasury_execution.execute || config.treasury_execution.reconcile {
            let intake = config
                .treasury_intake
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("treasury bindings are not configured"))?;
            for (id, binding) in intake.bindings(config)? {
                let mint = config
                    .mints
                    .iter()
                    .find(|m| m.id == id)
                    .ok_or_else(|| anyhow::anyhow!("treasury mint missing"))?
                    .clone();
                let transport = MintTransport::new(
                    mint.advertised_url.parse()?,
                    Duration::from_secs(config.deadline_seconds),
                    config.body_bytes,
                )
                .map_err(|_| anyhow::anyhow!("invalid treasury mint transport"))?;
                contexts.push((
                    Arc::new(TreasuryPolicy {
                        binding,
                        mint,
                        seed: *runtime.keys.seed,
                    }),
                    transport,
                ));
            }
        }
        let databases = runtime
            .mint_databases
            .iter()
            .map(|(id, database)| {
                let database: Arc<dyn WalletDatabase<DatabaseError> + Send + Sync> =
                    database.clone();
                (id.clone(), database)
            })
            .collect();
        Self::from_mint_stores(
            runtime.journal.clone(),
            databases,
            runtime.writer.clone(),
            Arc::new(runtime.policy.keys.clone()),
            contexts,
            config.treasury_execution.clone(),
            Duration::from_secs(config.deadline_seconds),
            config.body_bytes,
        )
    }
}
impl<T: Transport + Clone + 'static> Worker<T> {
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_parts(
        journal: Arc<Mutex<Journal>>,
        database: Arc<dyn WalletDatabase<DatabaseError> + Send + Sync>,
        writer: Arc<WriterLock>,
        keys: Arc<PinnedKeys>,
        policies: Vec<(Arc<TreasuryPolicy>, T)>,
        execution: Execution,
        deadline: Duration,
        message_limit: usize,
    ) -> Result<Self> {
        let databases = policies
            .iter()
            .map(|(policy, _)| (policy.mint.id.clone(), database.clone()))
            .collect();
        Self::from_mint_stores(
            journal,
            databases,
            writer,
            keys,
            policies,
            execution,
            deadline,
            message_limit,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_mint_stores(
        journal: Arc<Mutex<Journal>>,
        databases: BTreeMap<String, Arc<dyn WalletDatabase<DatabaseError> + Send + Sync>>,
        writer: Arc<WriterLock>,
        keys: Arc<PinnedKeys>,
        policies: Vec<(Arc<TreasuryPolicy>, T)>,
        execution: Execution,
        deadline: Duration,
        message_limit: usize,
    ) -> Result<Self> {
        execution.validate()?;
        ensure!(
            deadline.as_secs() >= 1 && deadline <= Duration::from_secs(60) && policies.len() <= 16,
            "invalid treasury worker bounds"
        );
        let mut contexts = BTreeMap::new();
        for (policy, transport) in policies {
            let database = databases
                .get(&policy.mint.id)
                .ok_or_else(|| anyhow::anyhow!("treasury mint namespace missing"))?
                .clone();
            let executor = if policy.mint.bolt11_exit_qualified {
                let mut split = execution.split.clone();
                split.enabled &= execution.execute;
                Some(
                    Executor::new(
                        transport.clone(),
                        policy.clone(),
                        database.clone(),
                        journal.clone(),
                        writer.clone(),
                        keys.clone(),
                        message_limit,
                    )?
                    .with_split_policy(Arc::new(split))?,
                )
            } else {
                None
            };
            ensure!(
                contexts
                    .insert(
                        policy.mint.id.clone(),
                        Context {
                            policy,
                            transport,
                            executor,
                            database,
                        }
                    )
                    .is_none(),
                "duplicate treasury worker mint"
            );
        }
        Ok(Self {
            journal,
            writer,
            keys,
            contexts,
            execution,
            deadline,
        })
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
    pub async fn tick(&self, now: i64) -> Result<Tick> {
        let work = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .claim_treasury_work(now, &self.execution, self.deadline.as_secs())?;
        let Some(work) = work else {
            return Ok(Tick::Idle);
        };
        let Some(context) = self.contexts.get(&work.mint_id) else {
            return Ok(Tick::UnconfiguredMint);
        };
        let result = tokio::time::timeout(self.deadline, self.step(context, &work, now)).await;
        Ok(if matches!(result, Ok(Ok(true))) {
            Tick::Progress
        } else {
            Tick::RetryRetained
        })
    }
    async fn step(&self, context: &Context<T>, work: &Work, now: i64) -> Result<bool> {
        let binding = &context.policy.binding;
        if work.action.executes() {
            ensure!(
                context.executor.is_some(),
                "treasury mint exit is not qualified"
            );
            self.journal
                .lock()
                .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
                .authorize_treasury_execution(binding, work.id, &self.execution, now)?;
        }
        let recovery = Recovery {
            transport: &context.transport,
            journal: self.journal.clone(),
            database: context.database.clone(),
            writer: self.writer.clone(),
            binding,
            keys: &self.keys,
            seed: &context.policy.seed,
        };
        match work.action {
            Action::Reserve => {
                let selected = {
                    let mut journal = self
                        .journal
                        .lock()
                        .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
                    journal.reserve_treasury_sources(binding, work.id, u64::try_from(now)?)?;
                    journal
                        .try_select_treasury_proofs(binding, work.id, u64::try_from(now)?)?
                        .is_some()
                };
                if !selected {
                    if !self.execution.split.enabled {
                        return Ok(false);
                    }
                    context
                        .executor
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("split executor unavailable"))?
                        .reserve_split(work.id)
                        .await?;
                }
            }
            Action::SelectSplit => {
                self.journal
                    .lock()
                    .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
                    .select_treasury_proofs(binding, work.id, u64::try_from(now)?)?;
            }
            Action::RecoverQuote => {
                return Ok(recovery
                    .recover_quote(work.id, &context.policy.mint, u64::try_from(now)?)
                    .await?
                    .is_some());
            }
            Action::Reconcile => return Ok(recovery.reconcile(work.id).await?.is_some()),
            Action::Cleanup => return recovery.cleanup(work.id, None).await.map(|_| true),
            action => {
                let executor = context
                    .executor
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("treasury preparation unavailable"))?;
                match action {
                    Action::Split => {
                        executor.execute_split(work.id).await?;
                    }
                    Action::RecoverSplit => {
                        return executor.reconcile_split(work.id).await;
                    }
                    Action::Quote => {
                        executor.quote_reserved(work.id).await?;
                    }
                    Action::Prepare => {
                        // The quote journal commits before CDK persists its row.
                        // Restore that exact row before consuming the one-time
                        // preparation claim; never request a replacement quote.
                        ensure!(
                            recovery
                                .recover_quote(work.id, &context.policy.mint, u64::try_from(now)?)
                                .await?
                                .is_some(),
                            "original quote is unavailable for preparation"
                        );
                        executor.prepare_reserved(work.id).await?;
                    }
                    Action::RecoverPreparation => {
                        executor.recover_preparation(work.id).await?;
                    }
                    Action::ResumePreparation => {
                        executor
                            .recover_preparation_with_policy(work.id, Some(&self.execution))
                            .await?;
                    }
                    Action::Submit => {
                        executor.resume_prepared(work.id).await?;
                    }
                    _ => anyhow::bail!("unsupported treasury action"),
                }
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_configuration_defaults_off_and_bounds_whole_proof_exposure() -> Result<()> {
        let defaults: Execution = serde_json::from_str("{}")?;
        assert!(!defaults.split.enabled);
        for value in [
            r#"{"split":{"enabled":true}}"#,
            r#"{"split":{"enabled":true,"limits":{"maximum_input_sats":16,"maximum_outputs":1025}}}"#,
            r#"{"split":{"unknown":true}}"#,
            r#"{"split":{"limits":{"unknown":1}}}"#,
        ] {
            assert!(
                serde_json::from_str::<Execution>(value)
                    .and_then(|p| p.validate().map_err(serde::de::Error::custom))
                    .is_err()
            );
        }
        let paused: Execution = serde_json::from_str(
            r#"{"reconcile":true,"maximum_attempts":4,"split":{"enabled":true,"limits":{"maximum_input_sats":16,"maximum_fee_sats":1,"maximum_outputs":8},"operations_per_24h":1,"input_sats_per_24h":16,"maximum_in_flight_sats":16}}"#,
        )?;
        paused.validate()?;
        assert!(!paused.execute && paused.split.enabled);
        Ok(())
    }

    #[test]
    fn execution_defaults_off_and_requires_recovery_and_financial_limits() -> Result<()> {
        let defaults: Execution = serde_json::from_str("{}")?;
        defaults.validate()?;
        assert!(!defaults.execute && !defaults.reconcile && defaults.maximum_attempts == 0);
        for text in [
            r#"{"execute":true}"#,
            r#"{"reconcile":true}"#,
            r#"{"maximum_attempts":10001}"#,
            r#"{"execute":true,"maximum_attempts":1,"maximum_input_sats":8,"operations_per_24h":1,"input_sats_per_24h":8,"maximum_in_flight_sats":8}"#,
        ] {
            assert!(serde_json::from_str::<Execution>(text)?.validate().is_err());
        }
        serde_json::from_str::<Execution>(r#"{"reconcile":true,"maximum_attempts":2}"#)?
            .validate()?;
        let mut open_count: Execution = serde_json::from_str(
            r#"{"execute":true,"reconcile":true,"maximum_attempts":1,"maximum_input_sats":8,"unlimited_operations_per_24h":true,"input_sats_per_24h":8,"maximum_in_flight_sats":8,"fee_sats_per_24h":25}"#,
        )?;
        open_count.validate()?;
        open_count.operations_per_24h = 1;
        assert!(open_count.validate().is_err());
        assert!(serde_json::from_str::<Execution>(r#"{"arbitrary_melt":true}"#).is_err());
        Ok(())
    }
}
