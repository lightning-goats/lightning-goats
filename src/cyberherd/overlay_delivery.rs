//! The private bridge accepts only member presentation, never fabricated money
//! or feeder event types. Core never forwards these records to its Nostr outbox.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub epoch: Uuid,
    pub owner: String,
    pub identity: String,
    pub category: String,
    pub message: String,
}
impl Message {
    pub fn validate(&self) -> Result<()> {
        crate::nostr_event::validate_public_key(&self.owner)?;
        crate::nostr_event::validate_hex(&self.identity, 32)?;
        if self.epoch.is_nil()
            || self.message.trim().is_empty()
            || self.message.len() > 32768
            || super::templates::audience(&self.category)
                != Some(super::templates::Audience::NostrAndOverlay)
            || matches!(
                self.category.as_str(),
                "payment_received"
                    | "feeder_confirmed"
                    | "sats_received"
                    | "sats_received_zap"
                    | "feeder_trigger"
                    | "herd_reset_message"
            )
        {
            bail!("invalid CyberHerd member overlay message");
        }
        Ok(())
    }
}
