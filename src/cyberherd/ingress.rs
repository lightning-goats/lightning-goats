//! Ordered private input. Settlement identity is independent of core sequence,
//! local SQLite IDs and Nostr request IDs. These DTOs confer no money authority;
//! only Store::ingest_core accepts the transport's authenticated capability.
use std::collections::BTreeMap;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{membership::validate_pubkey, store::Snapshot};
use crate::core_bridge::{Feeding, Payment};

pub const MAX_PENDING: i64 = 8192;

#[derive(Debug)]
pub struct IngressFull;
impl std::fmt::Display for IngressFull {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CyberHerd intake full; retain upstream cursor while draining work")
    }
}
impl std::error::Error for IngressFull {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CoreBinding {
    pub core_uid: u32,
    pub epoch: Uuid,
    /// Explicit activation boundary. No automatic historical payout/replay.
    pub start_after: u64,
    pub credit_pool: String,
    /// Root configuration maps actual receive addresses to accepted native
    /// payees. The payer's callback cannot select a different project roster.
    pub payees: BTreeMap<String, String>,
}

impl CoreBinding {
    pub fn validate(&self) -> Result<()> {
        if self.core_uid == 0
            || self.epoch.is_nil()
            || self.start_after > i64::MAX as u64
            || self.payees.is_empty()
            || self.payees.len() > 32
        {
            bail!("invalid CyberHerd core activation binding");
        }
        crate::domain::invoice::validate_user(&self.credit_pool)?;
        for (user, payee) in &self.payees {
            crate::domain::invoice::validate_user(user)?;
            crate::nostr_event::validate_public_key(payee)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCursor {
    pub binding: CoreBinding,
    pub received_through: u64,
}

#[derive(Debug, Clone)]
pub enum Input {
    Social(String),
    Payment(Box<Payment>),
    Feeding(Feeding),
}

#[derive(Debug, Clone)]
pub struct Pending {
    pub seq: i64,
    pub source_key: String,
    pub input: Input,
    pub was_deferred: bool,
}

/// Membership at a confirmed-feeding barrier, before any later-arriving input.
/// This is not a funded payout plan: current payee eligibility, real reserves,
/// recipient minima and fee budgets must still be frozen by the planner.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeedingBarrier {
    pub ingress_seq: i64,
    pub feeding: Feeding,
    pub snapshot: Snapshot,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub conversations: BTreeMap<String, super::conversation::MemberConversation>,
}

pub(crate) fn hash(value: &impl Serialize) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(value)?)))
}

pub(crate) fn payment_identity(payment: &Payment) -> Result<(String, String)> {
    let key = format!(
        "settlement:{}",
        hash(&(&payment.source, &payment.source_id))?
    );
    // Local row IDs/observation sequences can change after explicit restore
    // recovery. The actual financial/native-request facts cannot.
    let proof = hash(&(
        &payment.source,
        &payment.source_id,
        &payment.payment_hash,
        &payment.address_user,
        &payment.credit_pool,
        payment.amount_msat,
        payment.paid_at,
        &payment.zap,
    ))?;
    Ok((key, proof))
}

pub(crate) fn validate_owner(owner: &str) -> Result<()> {
    validate_pubkey(owner)?;
    crate::nostr_event::validate_public_key(owner)
}
