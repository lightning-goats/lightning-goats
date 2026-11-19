//! The private bridge accepts only member presentation, never fabricated money
//! or feeder event types. Core never forwards these records to its Nostr outbox.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InteractionCards {
    pub occurred_at: i64,
    pub member: String,
    pub roster: super::view::Roster,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub epoch: Uuid,
    pub owner: String,
    pub identity: String,
    pub category: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction_cards: Option<InteractionCards>,
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
        if let Some(cards) = &self.interaction_cards {
            cards.roster.validate(true)?;
            crate::nostr_event::validate_public_key(&cards.member)?;
            if cards.occurred_at < cards.roster.cycle_start || cards.roster.owner != self.owner {
                bail!("invalid interaction card origin");
            }
        }
        Ok(())
    }
}
