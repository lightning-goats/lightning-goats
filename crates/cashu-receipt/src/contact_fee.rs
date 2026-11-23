//! Private, purpose-bound admission identity for NUT-24 paid contact.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const FEE_SATS: u64 = 100;
pub const MAX_TOKEN_BYTES: usize = 8192;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactTender {
    pub contact_id: Uuid,
    pub descriptor_id: String,
}
impl ContactTender {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.contact_id.get_version_num() == 4
                && self
                    .descriptor_id
                    .strip_prefix("lg1-")
                    .is_some_and(|s| s.len() == 32
                        && s.bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))),
            "invalid contact tender identity"
        );
        Ok(())
    }
    pub fn source_ref(&self, delivery_id: Uuid) -> String {
        format!("contact_fee:{}:{delivery_id}", self.contact_id)
    }
    pub fn source_terms(source: &str) -> Result<Option<(Uuid, Uuid)>> {
        let Some(raw) = source.strip_prefix("contact_fee:") else {
            return Ok(None);
        };
        let (contact, delivery) = raw
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("invalid contact tender identity"))?;
        let contact: Uuid = contact.parse()?;
        let delivery: Uuid = delivery.parse()?;
        ensure!(
            contact.get_version_num() == 4
                && delivery.get_version_num() == 8
                && source == format!("contact_fee:{contact}:{delivery}"),
            "invalid contact tender identity"
        );
        Ok(Some((contact, delivery)))
    }
}
