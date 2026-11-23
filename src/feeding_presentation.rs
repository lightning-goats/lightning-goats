//! Best-effort display metadata; physical authority stays in the existing worker/gateway.
use crate::{config::RuntimeMode, gateway::GatewayClient, ledger::LedgerStore};
use anyhow::Result;
use std::time::Duration;

pub async fn run_feeding_presentation_worker(
    ledger: LedgerStore,
    gateway: GatewayClient,
    threshold: u64,
    mode: RuntimeMode,
) -> Result<()> {
    loop {
        let safety = match gateway.feeder_safety().await {
            Ok(safety) => Some(safety),
            Err(error) => {
                tracing::warn!(%error,"feeding message status unavailable; settlement is unaffected");
                None
            }
        };
        if let Err(error) = ledger
            .update_feeding_presentation(threshold, mode, safety)
            .await
        {
            tracing::warn!(%error,"feeding message cache update failed; settlement and physical safety are independent");
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// Independent read-only loop, so slow/missing Astro Items cannot delay the
/// two-second safety cache or the feeder. Older gateways simply use fallback.
pub async fn run_solar_presentation_worker(
    ledger: LedgerStore,
    gateway: GatewayClient,
) -> Result<()> {
    loop {
        if let Ok(context) = gateway.solar_context().await
            && ledger.update_solar_context(&context).await.is_err()
        {
            tracing::warn!("solar presentation cache unavailable");
        }
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}
