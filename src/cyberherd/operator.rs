//! Explicit audited membership commands. No core money or feeder capability.
use super::membership::{Profile, Settings};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub id: uuid::Uuid,
    pub expected_revision: i64,
    pub action: Action,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Settings {
        minimum_sats: String,
        max_members: u16,
        member_percent: u8,
    },
    Activate {
        pubkey: String,
        profile: ProfileInput,
    },
    Amount {
        pubkey: String,
        amount_sats: String,
    },
    Remove {
        pubkey: String,
    },
    Ban {
        pubkey: String,
    },
    Unban {
        pubkey: String,
    },
    Reset,
    Template {
        edit: super::store::TemplateEdit,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProfileInput {
    pub name: String,
    pub picture: Option<String>,
    pub lud16: Option<String>,
}
impl From<&ProfileInput> for Profile {
    fn from(v: &ProfileInput) -> Self {
        Self {
            name: v.name.clone(),
            picture: v.picture.clone(),
            lud16: v.lud16.clone(),
        }
    }
}
pub fn sats(value: &str) -> Result<u64> {
    let n = value.parse::<u64>()?;
    if n > i64::MAX as u64 || value != n.to_string() {
        bail!("amount requires a canonical whole-sat decimal");
    }
    Ok(n)
}
impl Command {
    pub fn validate(&self) -> Result<()> {
        if self.id.is_nil() || self.expected_revision < 0 {
            bail!("invalid operator command identity/revision");
        }
        match &self.action {
            Action::Settings {
                minimum_sats,
                max_members,
                member_percent,
            } => Settings {
                minimum_sats: sats(minimum_sats)?,
                max_members: *max_members,
                member_percent: *member_percent,
            }
            .validate()?,
            Action::Activate { pubkey, profile } => {
                crate::nostr_event::validate_public_key(pubkey)?;
                Profile::from(profile).validate()?;
            }
            Action::Amount {
                pubkey,
                amount_sats,
            } => {
                crate::nostr_event::validate_public_key(pubkey)?;
                sats(amount_sats)?;
            }
            Action::Remove { pubkey } | Action::Ban { pubkey } | Action::Unban { pubkey } => {
                crate::nostr_event::validate_public_key(pubkey)?
            }
            Action::Reset | Action::Template { .. } => {}
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub command_id: uuid::Uuid,
    pub cycle: i64,
    pub revision: i64,
    pub changed: bool,
    pub event_seq: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Notice {
    pub member: String,
    pub profile: Profile,
    pub category: String,
    pub snapshot: super::store::Snapshot,
    pub allocation_changed: bool,
    pub context: super::conversation::Context,
    pub conversation: Option<super::conversation::MemberConversation>,
    pub activation: bool,
}

#[cfg(test)]
#[path = "operator_tests.rs"]
mod tests;

/// Authenticated core high-water observation; never accepted from HTTP JSON.
pub(crate) struct CoreFence {
    epoch: uuid::Uuid,
    core_uid: u32,
    through: u64,
    observed_at: i64,
}
impl CoreFence {
    #[cfg(unix)]
    pub(crate) async fn observe(
        store: &super::store::Store,
        owner: &str,
        core: &crate::core_bridge::unix::Client,
    ) -> Result<Self> {
        let cursor = store.core_cursor(owner).await?;
        let page = core
            .events(Some(cursor.binding.epoch), cursor.received_through, 1)
            .await?;
        if page.core_uid() != cursor.binding.core_uid || page.latest != cursor.received_through {
            bail!("operator edit awaits core intake catch-up");
        }
        Ok(Self {
            epoch: page.epoch,
            core_uid: page.core_uid(),
            through: page.latest,
            observed_at: super::calendar::now()?,
        })
    }
    pub(crate) fn validate(&self, cursor: &super::ingress::CoreCursor, now: i64) -> Result<()> {
        if self.epoch != cursor.binding.epoch
            || self.core_uid != cursor.binding.core_uid
            || self.through != cursor.received_through
            || !(0..=5).contains(&now.saturating_sub(self.observed_at))
        {
            bail!("operator edit ordering observation changed or expired");
        }
        Ok(())
    }
}
pub(crate) struct Environment {
    pub feeder: super::membership::FeederAvailability,
    pub context: super::conversation::Context,
    pub ordering: Option<CoreFence>,
    pub now: i64,
}
