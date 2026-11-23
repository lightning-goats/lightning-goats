//! Confirmed-food presentation. Only authenticated core feeding proof reaches
//! this producer; no command result, relay event or balance threshold is food.
use crate::presentation::OverlayGoat;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Proof {
    pub owner: String,
    pub identity: String,
    pub source_seq: u64,
    pub triggering_payment_amount_sats: Option<u64>,
    pub feed_credit_sats: u64,
}
impl Proof {
    pub fn validate(&self) -> Result<()> {
        crate::nostr_event::validate_public_key(&self.owner)?;
        crate::nostr_event::validate_hex(&self.identity, 32)?;
        if self.source_seq == 0
            || self.source_seq > i64::MAX as u64
            || self.feed_credit_sats > i64::MAX as u64
            || self
                .triggering_payment_amount_sats
                .is_some_and(|n| n == 0 || n > i64::MAX as u64)
        {
            bail!("invalid confirmed-feeding presentation proof");
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Broadcast {
    pub request_id: Uuid,
    pub proof: Proof,
    pub goats: Vec<OverlayGoat>,
}
impl Broadcast {
    pub fn validate(&self, owner: &str, identity: &str) -> Result<()> {
        self.proof.validate()?;
        if self.request_id.is_nil()
            || self.proof.owner != owner
            || self.proof.identity != identity
            || self.goats.len() > 5
        {
            bail!("feeding broadcast scope changed");
        }
        for goat in &self.goats {
            let slug = match goat.name.as_str() {
                "Dexter" => "dexter",
                "Rowan" => "rowan",
                "Cosmo" => "cosmo",
                "Newton" => "newton",
                "Nova" => "nova",
                _ => bail!("invalid feeding overlay goat"),
            };
            if goat.image_url != format!("images/{slug}.png") {
                bail!("invalid feeding overlay artwork path");
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Overlay {
    pub epoch: Uuid,
    pub broadcast: Broadcast,
    pub message: String,
}
impl Overlay {
    pub fn validate(&self) -> Result<()> {
        self.broadcast
            .validate(&self.broadcast.proof.owner, &self.broadcast.proof.identity)?;
        if self.epoch.is_nil() || self.message.trim().is_empty() || self.message.len() > 32768 {
            bail!("invalid confirmed feeding overlay text");
        }
        Ok(())
    }
}
