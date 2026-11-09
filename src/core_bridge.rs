//! Private core/CyberHerd protocol. A protected, peer-authenticated transport is
//! required; deserializing a DTO by itself never establishes financial authority.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(unix)]
pub mod unix;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BridgeConfig {
    pub socket: std::path::PathBuf,
    pub peer_uid: u32,
    pub group_gid: u32,
    #[serde(default)]
    pub reward_policy: Option<crate::reward_funding::RewardPolicy>,
}

impl BridgeConfig {
    pub fn validate(&self) -> Result<()> {
        if let Some(policy) = &self.reward_policy {
            policy.validate()?;
        }
        if !self.socket.is_absolute()
            || self.socket.as_os_str().len() > 100
            || self.socket.file_name().is_none()
            || self.peer_uid == 0
            || self.group_gid == 0
            || self.socket.components().any(|part| {
                !matches!(
                    part,
                    std::path::Component::RootDir | std::path::Component::Normal(_)
                )
            })
        {
            bail!("core bridge requires a bounded absolute socket path and non-root peer/group");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Events {
        expected_epoch: Option<Uuid>,
        after: u64,
        limit: u16,
    },
    Safety,
    RewardAllowance {
        expected_epoch: Uuid,
        request_id: Uuid,
        owner: String,
    },
    ReserveReward {
        expected_epoch: Uuid,
        request_id: Uuid,
        owner: String,
        plan_hash: String,
        amount_sats: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum Response {
    Events {
        batch: Box<Batch>,
    },
    Safety {
        state: Safety,
    },
    Reservation {
        reservation: Box<crate::reward_funding::Reservation>,
    },
    Allowance {
        allowance: Box<crate::reward_funding::Allowance>,
    },
    Rejected,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Ready,
    Paused,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Safety {
    pub availability: Availability,
    pub observed_at: Option<i64>,
    pub nostr_enabled: bool,
}

impl Safety {
    pub fn current_availability(&self, now: i64) -> Availability {
        if now < 0
            || self
                .observed_at
                .is_none_or(|at| at > now || now.saturating_sub(at) > 5)
        {
            Availability::Unknown
        } else {
            self.availability
        }
    }
}

pub const VERSION: u8 = 2;
pub const MAX_FRAME: usize = 262_144;
pub const MAX_BATCH_BYTES: usize = 196_608;
pub const MAX_PAGE: u16 = 128;

/// Constructed only by the child Unix client after checking the actual peer
/// and the complete response. No Deserialize/From<Batch> escape hatch.
#[derive(Clone)]
pub struct AuthenticatedBatch {
    batch: Batch,
    core_uid: u32,
}

/// Private transport capability, never constructed from a persisted DTO.
#[derive(Clone)]
pub struct AuthenticatedReservation {
    reservation: crate::reward_funding::Reservation,
    core_uid: u32,
}

/// Constructible only after checking the kernel peer and complete scope.
#[derive(Clone)]
pub struct AuthenticatedAllowance {
    allowance: crate::reward_funding::Allowance,
    core_uid: u32,
}

impl AuthenticatedAllowance {
    pub fn allowance(&self) -> &crate::reward_funding::Allowance {
        &self.allowance
    }
    pub fn core_uid(&self) -> u32 {
        self.core_uid
    }
}

impl AuthenticatedReservation {
    pub fn reservation(&self) -> &crate::reward_funding::Reservation {
        &self.reservation
    }
    pub fn core_uid(&self) -> u32 {
        self.core_uid
    }
}

impl AuthenticatedBatch {
    pub fn batch(&self) -> &Batch {
        &self.batch
    }
    pub fn core_uid(&self) -> u32 {
        self.core_uid
    }
}

impl std::ops::Deref for AuthenticatedBatch {
    type Target = Batch;
    fn deref(&self) -> &Self::Target {
        &self.batch
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    pub version: u8,
    pub epoch: Uuid,
    pub after: u64,
    pub through: u64,
    pub latest: u64,
    pub events: Vec<CoreEvent>,
}

impl Batch {
    pub fn validate(&self, expected: Option<Uuid>, after: u64) -> Result<()> {
        if self.version != VERSION
            || self.epoch.is_nil()
            || expected.is_some_and(|id| id != self.epoch)
            || self.after != after
            || self.latest > i64::MAX as u64
            || self.through > self.latest
            || self.through < self.after
            || self.events.len() > usize::from(MAX_PAGE)
            || self.through - self.after != self.events.len() as u64
        {
            bail!("core batch identity or cursor is inconsistent");
        }
        for (offset, event) in self.events.iter().enumerate() {
            if event.seq != after + offset as u64 + 1 || event.observed_at < 0 {
                bail!("core event sequence is inconsistent");
            }
            match &event.body {
                EventBody::Payment(payment) => {
                    if payment.settlement_id <= 0
                        || payment.amount_msat == 0
                        || payment.amount_msat > i64::MAX as u64
                        || !payment.amount_msat.is_multiple_of(1000)
                        || payment.paid_at < 0
                        || payment.paid_at > event.observed_at.saturating_add(300)
                    {
                        bail!("invalid core settlement proof");
                    }
                    super::domain::invoice::validate_user(&payment.address_user)?;
                    super::domain::invoice::validate_user(&payment.credit_pool)?;
                    if payment.source.is_empty()
                        || payment.source.len() > 32
                        || payment.source_id.is_empty()
                        || payment.source_id.len() > 256
                    {
                        bail!("invalid core settlement source");
                    }
                    if let Some(hash) = &payment.payment_hash {
                        crate::nostr_event::validate_hex(hash, 32)?;
                    }
                    if let Some(zap) = &payment.zap {
                        let request = crate::zap::ZapRequest::validate(
                            &zap.request_json,
                            &zap.payee_pubkey,
                            &zap.receipt_pubkey,
                            payment.amount_msat,
                        )?;
                        if request.description_hash() != zap.description_hash {
                            bail!("core zap bytes do not match invoice hash");
                        }
                        let invoice: lightning_invoice::Bolt11Invoice = zap.invoice.parse()?;
                        crate::bolt11::verify(
                            &zap.invoice,
                            payment.amount_msat,
                            &invoice.payment_hash().to_string(),
                            &zap.description_hash,
                        )?;
                        if payment
                            .payment_hash
                            .as_ref()
                            .is_some_and(|hash| hash != &invoice.payment_hash().to_string())
                        {
                            bail!("core settlement hash contradicts issued invoice");
                        }
                    }
                }
                EventBody::Feeding(feed) => {
                    if let Some(funding) = &feed.funding {
                        funding.validate(feed.threshold_sats)?;
                    }
                    if feed.request_id.is_nil()
                        || feed.threshold_sats == 0
                        || feed.threshold_sats > i64::MAX as u64
                        || feed.confirmed_at < 0
                        || feed.confirmed_at > event.observed_at.saturating_add(300)
                    {
                        bail!("invalid core feeding proof");
                    }
                }
                EventBody::OperatorCredit {
                    amount_sats,
                    address_user,
                    credit_pool,
                } => {
                    if *amount_sats == 0 || *amount_sats > i64::MAX as u64 {
                        bail!("invalid virtual credit");
                    }
                    crate::domain::invoice::validate_user(address_user)?;
                    crate::domain::invoice::validate_user(credit_pool)?;
                }
                EventBody::Other => {}
            }
        }
        if serde_json::to_vec(self)?.len() > MAX_FRAME {
            bail!("core batch exceeds frame bound");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CoreEvent {
    pub seq: u64,
    pub observed_at: i64,
    pub body: EventBody,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EventBody {
    Payment(Box<Payment>),
    Feeding(Feeding),
    OperatorCredit {
        amount_sats: u64,
        address_user: String,
        credit_pool: String,
    },
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Payment {
    pub settlement_id: i64,
    pub source: String,
    pub source_id: String,
    pub payment_hash: Option<String>,
    pub address_user: String,
    pub credit_pool: String,
    pub amount_msat: u64,
    pub paid_at: i64,
    pub zap: Option<SettledZap>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SettledZap {
    pub request_json: String,
    pub payee_pubkey: String,
    pub receipt_pubkey: String,
    pub invoice: String,
    pub description_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Feeding {
    pub request_id: Uuid,
    pub threshold_sats: u64,
    pub confirmed_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub funding: Option<crate::reward_funding::Funding>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_and_wire_schema_do_not_offer_root_or_mutation_shortcuts() {
        let good = BridgeConfig {
            socket: "/run/example-bridge/core.sock".into(),
            peer_uid: 10001,
            group_gid: 10002,
            reward_policy: None,
        };
        good.validate().unwrap();
        for bad in [
            BridgeConfig {
                peer_uid: 0,
                ..good.clone()
            },
            BridgeConfig {
                group_gid: 0,
                ..good.clone()
            },
            BridgeConfig {
                socket: "relative.sock".into(),
                ..good.clone()
            },
            BridgeConfig {
                socket: "/run/../core.sock".into(),
                ..good
            },
        ] {
            assert!(bad.validate().is_err());
        }
        assert!(
            serde_json::from_str::<Request>(r#"{"method":"payment_received","amount_sats":1000}"#)
                .is_err()
        );
        assert!(
            serde_json::from_str::<Request>(
                r#"{"method":"events","after":0,"limit":1,"token":"fake"}"#
            )
            .is_err()
        );
        let state = Safety {
            availability: Availability::Ready,
            observed_at: Some(100),
            nostr_enabled: true,
        };
        assert_eq!(state.current_availability(100), Availability::Ready);
        assert_eq!(state.current_availability(106), Availability::Unknown);
        assert_eq!(state.current_availability(99), Availability::Unknown);
    }
}
