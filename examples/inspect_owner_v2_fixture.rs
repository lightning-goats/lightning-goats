//! Read-only acceptance probe for the fixed, unlinked HOME owner-v2 fixture.
//! Uses the named systemd credential. Never commands an Item or prints a secret.
use anyhow::{Context, Result};
use lightning_goats::openhab::{OpenHabClient, OwnerProtocol, TrustedOpenHabConfig};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    let id = Uuid::parse_str(
        &std::env::args()
            .nth(1)
            .context("supply existing fixture UUID")?,
    )?;
    let config = TrustedOpenHabConfig {
        url: "http://127.0.0.1:8080/".into(),
        request_item: "LightningGoatsOwnerV2TestRequest".into(),
        ack_item: "LightningGoatsOwnerV2TestResult".into(),
        protocol: OwnerProtocol::FeederRequestV2 {
            ledger_item: "LightningGoatsOwnerV2TestLedger".into(),
        },
        override_item: "LightningGoatsOwnerV2TestActuator".into(),
        remote_enabled_item: "LightningGoatsOwnerV2TestBootstrap".into(),
        temperature_item: None,
    };
    let client = OpenHabClient::from_config(&config).await?;
    println!("{:?}", client.feeder_result(id).await?);
    Ok(())
}
