#![forbid(unsafe_code)]
use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    about = "Separate CyberHerd membership, optional presentation and operator service (no payouts)"
)]
struct Args {
    #[arg(long, default_value = "/etc/cyberherd/config.toml")]
    config: PathBuf,
    /// Validate configuration without opening databases or contacting services.
    #[arg(long, conflicts_with_all = ["initialize_state", "record_initial_reward_cutover"])]
    check: bool,
    /// Initialize only a fresh disabled journal, without credentials or networking.
    #[arg(long, conflicts_with = "record_initial_reward_cutover")]
    initialize_state: bool,
    /// Offline root-reviewed historical reward disposition; never starts workers.
    #[arg(long, requires_all = ["reward_cutover_sha256", "writers_stopped"])]
    record_initial_reward_cutover: Option<PathBuf>,
    /// Independently reviewed SHA256 of the protected preview file.
    #[arg(long, requires = "record_initial_reward_cutover")]
    reward_cutover_sha256: Option<String>,
    /// Attest that the parent transaction has stopped all four journal writers.
    #[arg(long, requires = "record_initial_reward_cutover")]
    writers_stopped: bool,
}

#[cfg(unix)]
#[tokio::main]
async fn main() -> Result<()> {
    use lightning_goats::cyberherd::runtime::{Config, Runtime};
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args = Args::parse();
    let config = Config::load(&args.config)?;
    if args.check {
        println!("CyberHerd configuration is valid; no runtime started");
        return Ok(());
    }
    if args.initialize_state {
        config.initialize_state(&args.config).await?;
        println!("New CyberHerd journal initialized; no runtime started");
        return Ok(());
    }
    if let Some(preview) = args.record_initial_reward_cutover {
        let receipt = config
            .record_initial_reward_cutover(
                &args.config,
                &preview,
                args.reward_cutover_sha256.as_deref().unwrap_or_default(),
                args.writers_stopped,
            )
            .await?;
        println!("{}", serde_json::to_string(&receipt)?);
        return Ok(());
    }
    Runtime::open(config, &args.config).await?.run().await
}

#[cfg(not(unix))]
fn main() -> Result<()> {
    anyhow::bail!("CyberHerd requires Unix peer authentication")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cutoff_cli_requires_pin_and_stopped_writer_attestation_and_conflicts_with_other_modes() {
        let args = [
            "cyberherdd",
            "--record-initial-reward-cutover",
            "/protected/PREVIEW.json",
            "--reward-cutover-sha256",
            "abcd",
            "--writers-stopped",
        ];
        assert!(Args::try_parse_from(args).is_ok());
        assert!(Args::try_parse_from(&args[..3]).is_err());
        assert!(Args::try_parse_from(&args[..5]).is_err());
        for other in ["--check", "--initialize-state"] {
            assert!(Args::try_parse_from(args.into_iter().chain([other])).is_err());
        }
        assert!(Args::try_parse_from(["cyberherdd", "--writers-stopped"]).is_err());
    }
}
