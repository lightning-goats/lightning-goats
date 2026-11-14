//! Payout-role attestations are private journal reports, not public settlement
//! messages. Core independently reads every reported provider payment.
use super::{
    funding::ProviderScope,
    provider::{BindingRecord, PaymentState},
};
use crate::reward_funding::Reservation;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UnsubmittedReason {
    Unavailable,
    Expired,
    OperatorCancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Disposition {
    Unsubmitted {
        reason: UnsubmittedReason,
    },
    Payment {
        binding_json: String,
        state: PaymentState,
        fee_sats: u64,
        total_sats: u64,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub member: String,
    pub gross_sats: u64,
    pub disposition: Disposition,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub reservation: Reservation,
    pub scope: ProviderScope,
    pub rows: u16,
    pub final_hash: String,
}
impl Header {
    pub fn validate(&self) -> Result<()> {
        self.reservation.validate()?;
        crate::nostr_event::validate_hex(&self.scope.credential_fingerprint, 32)?;
        crate::nostr_event::validate_hex(&self.final_hash, 32)?;
        if self.scope.environment != "strike-production"
            || self.scope.account_id.is_nil()
            || !(1..=1000).contains(&self.rows)
        {
            bail!("invalid private payout outcome header");
        }
        Ok(())
    }
    pub fn seed(&self) -> Result<String> {
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(&(
            &self.reservation,
            &self.scope,
            self.rows,
        ))?)))
    }
}
pub fn next_hash(before: &str, row: &Row) -> Result<String> {
    crate::nostr_event::validate_hex(before, 32)?;
    let raw = serde_json::to_vec(row)?;
    if raw.len() > 24576 {
        bail!("outcome row exceeds private frame budget");
    }
    let mut hash = Sha256::new();
    hash.update(hex::decode(before)?);
    hash.update((raw.len() as u64).to_be_bytes());
    hash.update(raw);
    Ok(hex::encode(hash.finalize()))
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Packet {
    pub header: Header,
    pub after: u16,
    pub before_hash: String,
    pub row: Row,
}
impl Packet {
    pub(crate) fn validate(&self) -> Result<Option<BindingRecord>> {
        self.header.validate()?;
        crate::nostr_event::validate_public_key(&self.row.member)?;
        crate::nostr_event::validate_hex(&self.before_hash, 32)?;
        if self.after >= self.header.rows
            || self.row.member == self.header.reservation.owner
            || self.row.gross_sats == 0
            || self.row.gross_sats > self.header.reservation.reserved_sats
            || serde_json::to_vec(self)?.len() > crate::core_bridge::MAX_BATCH_BYTES
        {
            bail!("invalid payout outcome row scope/amount");
        }
        next_hash(&self.before_hash, &self.row)?;
        match &self.row.disposition {
            Disposition::Unsubmitted { .. } => Ok(None),
            Disposition::Payment {
                binding_json,
                state,
                fee_sats,
                total_sats,
            } => {
                if binding_json.len() > 16384 || *state == PaymentState::Pending {
                    bail!("pending/oversized payout cannot finalize");
                }
                let binding: BindingRecord = serde_json::from_str(binding_json)
                    .map_err(|_| anyhow::anyhow!("invalid payout correlation evidence"))?;
                binding.quote.validate()?;
                if binding.quote.request.scope != self.header.scope
                    || binding.payment_id.is_nil()
                    || binding.quote.request.gross_sats != self.row.gross_sats
                    || binding.quote.request.created_at < self.header.reservation.created_at
                    || *fee_sats > binding.quote.fee_sats
                    || binding.quote.request.amount_sats.checked_add(*fee_sats) != Some(*total_sats)
                    || *total_sats > self.row.gross_sats
                {
                    bail!("payout correlation contradicts reservation");
                }
                Ok(Some(binding))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    pub reservation_id: Uuid,
    pub epoch: Uuid,
    pub manifest_hash: String,
    pub through: u16,
    pub chain_hash: String,
    pub finalized: bool,
    pub spent_sats: u64,
    pub held_sats: u64,
    pub released_sats: u64,
    pub verified_at: i64,
}
impl Progress {
    pub fn validate(&self, header: &Header) -> Result<()> {
        header.validate()?;
        crate::nostr_event::validate_hex(&self.chain_hash, 32)?;
        let accounted = self
            .spent_sats
            .checked_add(self.held_sats)
            .and_then(|n| n.checked_add(self.released_sats))
            .context("outcome sum overflow")?;
        if self.reservation_id != header.reservation.id
            || self.epoch != header.reservation.epoch
            || self.manifest_hash != header.final_hash
            || self.through > header.rows
            || self.verified_at < header.reservation.created_at
            || accounted > header.reservation.reserved_sats
            || (self.finalized
                && (self.through != header.rows
                    || self.chain_hash != header.final_hash
                    || accounted != header.reservation.reserved_sats))
            || (!self.finalized && self.released_sats != 0)
        {
            bail!("invalid authenticated outcome progress");
        }
        Ok(())
    }
}

pub struct Reader {
    client: crate::strike::StrikeClient,
    account_id: Uuid,
}
impl Reader {
    pub(crate) fn account_id(&self) -> Uuid {
        self.account_id
    }
    pub fn new(client: crate::strike::StrikeClient, account_id: Uuid) -> Result<Self> {
        if account_id.is_nil() {
            bail!("outcome verification requires pinned account label");
        }
        Ok(Self { client, account_id })
    }
    pub(crate) async fn verify(&self, packet: &Packet) -> Result<VerifiedRow> {
        let binding = packet.validate()?;
        if packet.header.scope.account_id != self.account_id {
            bail!("outcome account differs from read-only core binding");
        }
        let (spent, held, payment_id, invoice_hash) = if let Some(binding) = binding {
            let actual = self.client.verify_payout_payment(&binding).await?;
            let Disposition::Payment {
                state,
                fee_sats,
                total_sats,
                ..
            } = &packet.row.disposition
            else {
                unreachable!()
            };
            if actual.state != *state
                || actual.fee_sats != *fee_sats
                || actual.total_sats != *total_sats
            {
                bail!("provider has not verified the reported terminal state");
            }
            // Failed payments never claim net delivery; costs not proved refunded
            // remain held indefinitely instead of silently aging out as spent.
            let (spent, held) = if actual.state == PaymentState::Completed {
                (actual.total_sats, 0)
            } else {
                (0, packet.row.gross_sats)
            };
            (
                spent,
                held,
                Some(binding.payment_id),
                Some(binding.quote.request.payment_hash),
            )
        } else {
            (0, 0, None, None)
        };
        Ok(VerifiedRow {
            packet: packet.clone(),
            spent,
            held,
            payment_id,
            invoice_hash,
        })
    }
}
/// Only the read-only verifier can create this capability. The ledger repeats
/// reservation/epoch/cursor checks when committing, with no HTTP inside its tx.
pub(crate) struct VerifiedRow {
    packet: Packet,
    spent: u64,
    held: u64,
    payment_id: Option<Uuid>,
    invoice_hash: Option<String>,
}
impl VerifiedRow {
    pub(crate) fn packet(&self) -> &Packet {
        &self.packet
    }
    pub(crate) fn spent(&self) -> u64 {
        self.spent
    }
    pub(crate) fn held(&self) -> u64 {
        self.held
    }
    pub(crate) fn payment_id(&self) -> Option<Uuid> {
        self.payment_id
    }
    pub(crate) fn invoice_hash(&self) -> Option<&str> {
        self.invoice_hash.as_deref()
    }
}
