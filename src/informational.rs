use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::time::sleep;

use crate::{
    config::InformationalConfig,
    gateway::{GatewayClient, format_weather_message},
    ledger::LedgerStore,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InformationalChoice {
    None,
    InterfaceInfo,
    Weather,
}

pub async fn run_informational_worker(
    ledger: LedgerStore,
    gateway: GatewayClient,
    config: InformationalConfig,
) -> Result<()> {
    let interval = Duration::from_secs(config.interval_seconds);
    loop {
        sleep(interval).await;
        if let Err(error) = run_informational_cycle(&ledger, &gateway, &config).await {
            tracing::warn!(%error, "periodic informational cycle failed; payment and feeder processing are unaffected");
        }
    }
}

pub async fn run_informational_cycle(
    ledger: &LedgerStore,
    gateway: &GatewayClient,
    config: &InformationalConfig,
) -> Result<InformationalChoice> {
    let safety = gateway
        .feeder_safety()
        .await
        .context("trusted gateway safety unavailable for informational cycle")?;
    if safety.override_enabled {
        tracing::debug!("FeederOverride is ON; periodic overlay information suppressed");
        return Ok(InformationalChoice::None);
    }

    let cycle = current_cycle(config.interval_seconds)?;
    let choice = choose_for_cycle(cycle, config)?;
    match choice {
        InformationalChoice::None => {}
        InformationalChoice::InterfaceInfo => {
            ledger.append_event("interface_info", &json!({})).await?;
        }
        InformationalChoice::Weather => {
            let weather = gateway.weather().await?;
            let message = format_weather_message(&weather);
            ledger
                .append_event(
                    "weather_status",
                    &json!({
                        "message": message,
                        "data": weather
                    }),
                )
                .await?;
        }
    }
    Ok(choice)
}

pub fn choose_for_cycle(cycle: u64, config: &InformationalConfig) -> Result<InformationalChoice> {
    validate_probability(config.interface_info_probability)?;
    validate_probability(config.weather_probability)?;
    if config.interface_info_probability + config.weather_probability > 1.0 {
        bail!("informational unconditional probabilities must not exceed 1");
    }

    if config.interface_info_enabled
        && stable_draw(cycle, "interface-info") < config.interface_info_probability
    {
        return Ok(InformationalChoice::InterfaceInfo);
    }

    if !config.weather_enabled || config.weather_probability == 0.0 {
        return Ok(InformationalChoice::None);
    }

    // Preserve the legacy ordering semantics: interface info gets first chance,
    // then weather's conditional chance is raised so its *unconditional* chance
    // remains weather_probability when both categories are enabled.
    let conditional_weather_probability = if config.interface_info_enabled {
        let denominator = 1.0 - config.interface_info_probability;
        if denominator <= 0.0 {
            0.0
        } else {
            (config.weather_probability / denominator).min(1.0)
        }
    } else {
        config.weather_probability
    };

    if stable_draw(cycle, "weather") < conditional_weather_probability {
        Ok(InformationalChoice::Weather)
    } else {
        Ok(InformationalChoice::None)
    }
}

fn current_cycle(interval_seconds: u64) -> Result<u64> {
    if interval_seconds == 0 {
        bail!("informational interval must be greater than zero");
    }
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_secs();
    Ok(seconds / interval_seconds)
}

fn stable_draw(cycle: u64, purpose: &str) -> f64 {
    let mut hasher = Sha256::new();
    hasher.update(b"lightning-goats-info-v1");
    hasher.update(cycle.to_be_bytes());
    hasher.update(purpose.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    // Divide by 2^64 so the result is always in [0, 1).
    (u64::from_be_bytes(bytes) as f64) / ((u64::MAX as f64) + 1.0)
}

fn validate_probability(value: f64) -> Result<()> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        bail!("informational probability must be finite and between 0 and 1");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> InformationalConfig {
        InformationalConfig {
            interval_seconds: 60,
            interface_info_enabled: true,
            weather_enabled: true,
            interface_info_probability: 0.4,
            weather_probability: 0.4,
        }
    }

    #[test]
    fn choice_is_stable_for_same_cycle() {
        let config = defaults();
        for cycle in 0..1_000 {
            assert_eq!(
                choose_for_cycle(cycle, &config).unwrap(),
                choose_for_cycle(cycle, &config).unwrap()
            );
        }
    }

    #[test]
    fn exactly_one_category_can_win_per_cycle() {
        let config = defaults();
        for cycle in 0..10_000 {
            assert!(matches!(
                choose_for_cycle(cycle, &config).unwrap(),
                InformationalChoice::None
                    | InformationalChoice::InterfaceInfo
                    | InformationalChoice::Weather
            ));
        }
    }

    #[test]
    fn disabling_categories_prevents_selection() {
        let mut config = defaults();
        config.interface_info_enabled = false;
        config.weather_enabled = false;
        for cycle in 0..100 {
            assert_eq!(
                choose_for_cycle(cycle, &config).unwrap(),
                InformationalChoice::None
            );
        }
    }
}
