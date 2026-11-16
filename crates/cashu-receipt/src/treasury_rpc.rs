//! Private core-to-wallet treasury admission and inspection, never public spending RPC.
use super::treasury::{Intent, Issued, MAX_TREASURY_BYTES, MeltData, hex_digest};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u8,
    pub request_id: Uuid,
    pub command: Command,
}
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "action",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Command {
    Retain(Box<Issued>),
    Status(Box<Intent>),
}
impl Request {
    pub fn intent(&self) -> &Intent {
        match &self.command {
            Command::Retain(issued) => &issued.intent,
            Command::Status(intent) => intent,
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1 && !self.request_id.is_nil(),
            "invalid treasury RPC identity"
        );
        self.intent().validate()
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let bytes = serde_json::to_vec(self)?;
        ensure!(
            bytes.len() <= MAX_TREASURY_BYTES,
            "treasury RPC exceeds frame bound"
        );
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(
            !bytes.is_empty() && bytes.len() <= MAX_TREASURY_BYTES,
            "treasury RPC exceeds frame bound"
        );
        let value: Self = serde_json::from_slice(bytes)
            .map_err(|_| anyhow::anyhow!("invalid treasury RPC frame"))?;
        value.validate()?;
        Ok(value)
    }
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Bound,
    Prepared,
    Submitted,
    Reconciled,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub version: u8,
    pub request_id: Uuid,
    pub intent_id: Uuid,
    pub intent_digest: String,
    pub phase: Phase,
    pub report: Option<MeltData>,
}
impl Reply {
    pub fn validate_for(&self, request: &Request) -> Result<()> {
        request.validate()?;
        ensure!(
            self.version == 1
                && self.request_id == request.request_id
                && self.intent_id == request.intent().id
                && self.intent_digest == hex_digest(request.intent().digest()?),
            "treasury RPC reply binding mismatch"
        );
        ensure!(
            (self.phase == Phase::Reconciled) == self.report.is_some(),
            "treasury RPC report/state mismatch"
        );
        if let Some(report) = &self.report {
            report.validate(request.intent())?;
        }
        Ok(())
    }
    pub fn decode_for(bytes: &[u8], request: &Request) -> Result<Self> {
        ensure!(
            !bytes.is_empty() && bytes.len() <= MAX_TREASURY_BYTES,
            "treasury RPC reply exceeds bound"
        );
        let value: Self = serde_json::from_slice(bytes)
            .map_err(|_| anyhow::anyhow!("invalid treasury RPC reply"))?;
        value.validate_for(request)?;
        Ok(value)
    }
}
