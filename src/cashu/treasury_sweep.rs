//! Protected treasury sweep triggers; none grants public spending authority.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    pub enabled: bool,
    /// Zero disables the trigger. Age refers to retained available acquisition custody.
    pub maximum_holding_age_seconds: u64,
    /// Zero disables the per-mint available-custody balance trigger.
    pub balance_trigger_sats: u64,
    /// Stable operator-selected UUIDs; one original job at most for each request.
    pub requests: Vec<Request>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: Uuid,
    pub mint_id: String,
    pub maximum_input_sats: u64,
}
impl Policy {
    pub fn validate_shape(&self) -> Result<()> {
        ensure!(
            self.maximum_holding_age_seconds <= 31_536_000
                && self.balance_trigger_sats <= i64::MAX as u64
                && self.requests.len() <= 16,
            "invalid treasury sweep limits"
        );
        ensure!(
            !self.enabled
                || self.maximum_holding_age_seconds > 0
                || self.balance_trigger_sats > 0
                || !self.requests.is_empty(),
            "enabled sweeps require an explicit trigger"
        );
        let mut ids = std::collections::BTreeSet::new();
        for request in &self.requests {
            ensure!(
                !request.id.is_nil()
                    && ids.insert(request.id)
                    && request.maximum_input_sats > 0
                    && request.maximum_input_sats <= i64::MAX as u64
                    && !request.mint_id.is_empty()
                    && request.mint_id.len() <= 128
                    && !request.mint_id.chars().any(char::is_control),
                "invalid operator treasury sweep request"
            );
        }
        Ok(())
    }
    pub fn validate(&self, treasury: &super::treasury::Policy) -> Result<()> {
        self.validate_shape()?;
        ensure!(
            !(self.enabled && treasury.enabled)
                || self
                    .requests
                    .iter()
                    .all(|r| treasury.approved_mints.contains_key(&r.mint_id)),
            "operator sweep mint is not approved"
        );
        Ok(())
    }
}
