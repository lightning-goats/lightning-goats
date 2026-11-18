//! Protected treasury authority. The public token receiver cannot construct a
//! destination or turn a mint response into independently verified Strike funds.
use anyhow::{Result, ensure};
use lightning_goats_cashu_receipt::treasury::{Destination, Intent, MAX_TREASURY_BYTES, MeltData};
use serde::Deserialize;
use std::collections::BTreeMap;

use super::receipt_authority::{KernelPeer, ReceiptPolicy};

pub(crate) const TREASURY_USER: &str = "cashu_treasury";

#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub minimum_deposit_sats: u64,
    pub maximum_deposit_sats: u64,
    pub maximum_input_sats: u64,
    pub maximum_fee_sats: u64,
    pub operations_per_24h: u64,
    pub input_sats_per_24h: u64,
    pub unlimited_input_sats_per_24h: bool,
    pub fee_sats_per_24h: Option<u64>,
    pub maximum_in_flight_sats: u64,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MintLimits {
    pub minimum_deposit_sats: u64,
    pub maximum_deposit_sats: u64,
    pub maximum_input_sats: u64,
    pub maximum_fee_sats: u64,
    pub maximum_in_flight_sats: u64,
}

/// Separate from receipt import, discovery and reward permission. Example
/// configuration must leave this disabled with zero financial authority.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    #[serde(default)]
    pub enabled: bool,
    pub version: String,
    pub destination: Destination,
    #[serde(default)]
    pub approved_mints: BTreeMap<String, MintLimits>,
    pub invoice_expiry_seconds: u64,
    #[serde(default)]
    pub limits: Limits,
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        self.destination.validate()?;
        let bounded_label =
            |v: &str| !v.is_empty() && v.len() <= 128 && !v.chars().any(char::is_control);
        ensure!(
            bounded_label(&self.version)
                && (1..=3_600).contains(&self.invoice_expiry_seconds)
                && self.approved_mints.len() <= 16,
            "invalid treasury policy scope"
        );
        let l = &self.limits;
        ensure!(
            l.operations_per_24h <= 10_000
                && l.input_sats_per_24h <= i64::MAX as u64
                && (!l.unlimited_input_sats_per_24h || l.input_sats_per_24h == 0)
                && l.fee_sats_per_24h.is_none_or(|fee| fee <= i64::MAX as u64),
            "invalid treasury daily limits"
        );
        validate_limits(
            self.enabled,
            l.minimum_deposit_sats,
            l.maximum_deposit_sats,
            l.maximum_input_sats,
            l.maximum_fee_sats,
            l.maximum_in_flight_sats,
        )?;
        ensure!(
            !self.enabled
                || (!self.approved_mints.is_empty()
                    && l.operations_per_24h > 0
                    && (l.input_sats_per_24h > 0 || l.unlimited_input_sats_per_24h)),
            "enabled treasury requires explicit admission limits"
        );
        for (id, m) in &self.approved_mints {
            ensure!(bounded_label(id), "invalid treasury mint identity");
            validate_limits(
                self.enabled,
                m.minimum_deposit_sats,
                m.maximum_deposit_sats,
                m.maximum_input_sats,
                m.maximum_fee_sats,
                m.maximum_in_flight_sats,
            )?;
        }
        Ok(())
    }

    pub(crate) fn check_new(&self, intent: &Intent) -> Result<()> {
        self.validate()?;
        ensure!(self.enabled, "Cashu treasury spending disabled");
        intent.validate()?;
        let mint = self
            .approved_mints
            .get(&intent.mint_id)
            .ok_or_else(|| anyhow::anyhow!("treasury mint not approved"))?;
        for bound in [
            mint.minimum_deposit_sats,
            mint.maximum_deposit_sats,
            mint.maximum_input_sats,
            mint.maximum_in_flight_sats,
        ] {
            ensure!(
                bound > 0 && bound <= i64::MAX as u64,
                "mint treasury limits disabled or invalid"
            );
        }
        let limits = &self.limits;
        for bound in [
            limits.minimum_deposit_sats,
            limits.maximum_deposit_sats,
            limits.maximum_input_sats,
            limits.operations_per_24h,
            limits.maximum_in_flight_sats,
        ] {
            ensure!(
                bound > 0 && bound <= i64::MAX as u64,
                "treasury limits are disabled or invalid"
            );
        }
        ensure!(
            intent.treasury_policy == self.version
                && intent.destination == self.destination
                && intent.invoice_expiry_seconds == self.invoice_expiry_seconds
                && intent.deposit_sats >= limits.minimum_deposit_sats
                && intent.deposit_sats <= limits.maximum_deposit_sats
                && intent.input_sats <= limits.maximum_input_sats
                && intent.maximum_fee_sats <= limits.maximum_fee_sats,
            "treasury intent exceeds protected policy"
        );
        ensure!(
            intent.deposit_sats >= mint.minimum_deposit_sats
                && intent.deposit_sats <= mint.maximum_deposit_sats
                && intent.input_sats <= mint.maximum_input_sats
                && intent.maximum_fee_sats <= mint.maximum_fee_sats,
            "treasury intent exceeds mint limits"
        );
        Ok(())
    }
}

impl Limits {
    pub(crate) fn daily_input_remaining(&self, used: u64) -> u64 {
        if self.unlimited_input_sats_per_24h {
            u64::MAX
        } else {
            self.input_sats_per_24h.saturating_sub(used)
        }
    }
}

fn validate_limits(
    enabled: bool,
    minimum: u64,
    deposit: u64,
    input: u64,
    fee: u64,
    in_flight: u64,
) -> Result<()> {
    ensure!(
        [minimum, deposit, input, fee, in_flight]
            .into_iter()
            .all(|v| v <= i64::MAX as u64),
        "invalid treasury amount limit"
    );
    ensure!(
        !enabled
            || (minimum > 0
                && minimum <= deposit
                && deposit <= input
                && fee <= input
                && in_flight > 0),
        "invalid enabled treasury limits"
    );
    Ok(())
}

/// Authentication precedes parsing; duplicate terminal reports remain readable
/// while new spending/import are disabled. No constructor from claimed UIDs.
pub struct PeerMelt {
    data: MeltData,
    wallet_instance: uuid::Uuid,
    wallet_epoch: uuid::Uuid,
    core_epoch: uuid::Uuid,
    wallet_uid: u32,
}
impl PeerMelt {
    pub fn authenticate(
        registration: &ReceiptPolicy,
        peer: &KernelPeer,
        bytes: &[u8],
    ) -> Result<Self> {
        registration.check_peer(peer)?;
        ensure!(
            bytes.len() <= MAX_TREASURY_BYTES,
            "treasury report exceeds frame bound"
        );
        let data = serde_json::from_slice(bytes)
            .map_err(|_| anyhow::anyhow!("invalid private treasury report"))?;
        Ok(Self {
            data,
            wallet_instance: registration.wallet_instance,
            wallet_epoch: registration.wallet_epoch,
            core_epoch: registration.core_epoch,
            wallet_uid: registration.wallet_uid,
        })
    }
    pub(crate) fn validate(
        &self,
        registration: &ReceiptPolicy,
        intent: &Intent,
    ) -> Result<&MeltData> {
        ensure!(
            self.wallet_uid == registration.wallet_uid
                && self.wallet_instance == registration.wallet_instance
                && self.wallet_epoch == registration.wallet_epoch
                && self.core_epoch == registration.core_epoch
                && intent.wallet_instance == self.wallet_instance
                && intent.wallet_epoch == self.wallet_epoch
                && intent.core_epoch == self.core_epoch,
            "treasury wallet registration changed"
        );
        self.data.validate(intent)?;
        Ok(&self.data)
    }
    pub(crate) fn intent_id(&self) -> uuid::Uuid {
        self.data.intent_id
    }
}
