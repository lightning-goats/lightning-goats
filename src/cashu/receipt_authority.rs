//! Pretransaction receipt checks for the core-side importer. This does not grant
//! credit: budget reads/consumption, replay lookup, provenance and events must
//! still occur atomically in the core ledger transaction.
use anyhow::{Result, ensure};
use lightning_goats_cashu_receipt::WireReceipt;
use std::collections::BTreeMap;
use tokio::net::UnixStream;
use uuid::Uuid;

/// No public constructor from integers or serialized claims.
pub struct KernelPeer {
    sender_uid: u32,
    receiver_uid: u32,
}
impl KernelPeer {
    pub fn from_socket(socket: &UnixStream) -> Result<Self> {
        Ok(Self {
            sender_uid: socket.peer_cred()?.uid(),
            receiver_uid: crate::private_ipc::uid()?,
        })
    }
}

pub struct MintImportLimit {
    pub maximum_gross_sats: u64,
    pub maximum_net_sats: u64,
}

/// Protected core configuration, independent of the wallet's receiving toggles.
pub struct ReceiptPolicy {
    pub wallet_uid: u32,
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    pub core_epoch: Uuid,
    pub import_enabled: bool,
    pub limits: ImportLimits,
    /// Keep historical admitted mint/policy pairs available for reconciliation.
    pub approved: BTreeMap<(String, String), MintImportLimit>,
}

/// Zero defaults grant no new import authority. These are core limits, not
/// wallet-supplied amounts. Outstanding custody includes above-cap ecash.
#[derive(Default)]
pub struct ImportLimits {
    pub receipts_per_24h: u64,
    pub unlimited_receipts_per_24h: bool,
    pub credit_sats_per_24h: u64,
    pub maximum_custody_sats: u64,
}

/// These remaining values must be read under the same core writer transaction
/// that consumes them. A successful check is not a reservation of this budget.
#[derive(Default)]
pub struct RemainingBudget {
    pub receipt_count: u64,
    pub credit_sats: u64,
    pub custody_sats: u64,
}

/// Authenticated and structurally valid data, still requiring core replay and
/// transactional financial checks. Deliberately no Deserialize or Debug.
pub struct PeerReceipt {
    receipt: WireReceipt,
    wallet_uid: u32,
    core_epoch: Uuid,
}
impl PeerReceipt {
    pub fn receipt(&self) -> &WireReceipt {
        &self.receipt
    }
}

impl ReceiptPolicy {
    pub(crate) fn check_peer(&self, peer: &KernelPeer) -> Result<()> {
        ensure!(
            self.wallet_uid > 0
                && peer.sender_uid == self.wallet_uid
                && peer.receiver_uid > 0
                && peer.sender_uid != peer.receiver_uid,
            "wallet peer not authorized"
        );
        ensure!(
            !self.wallet_instance.is_nil()
                && !self.wallet_epoch.is_nil()
                && !self.core_epoch.is_nil(),
            "wallet registration not configured"
        );
        Ok(())
    }

    /// Also usable for duplicate/acknowledgment lookup while new import is off.
    /// Never accept a UID, instance or epoch asserted by an HTTP caller as authority.
    pub fn authenticate(&self, peer: &KernelPeer, bytes: &[u8]) -> Result<PeerReceipt> {
        self.check_peer(peer)?;
        let receipt = WireReceipt::decode(bytes)?;
        ensure!(
            receipt.data().wallet_instance == self.wallet_instance
                && receipt.data().wallet_epoch == self.wallet_epoch,
            "wallet registration/epoch mismatch"
        );
        Ok(PeerReceipt {
            receipt,
            wallet_uid: peer.sender_uid,
            core_epoch: self.core_epoch,
        })
    }

    /// The caller must first resolve an exact existing inbox record and return
    /// its original result. A conflict must fail without granting fresh credit.
    pub fn check_new(
        &self,
        authenticated: &PeerReceipt,
        remaining: &RemainingBudget,
    ) -> Result<()> {
        ensure!(self.import_enabled, "Cashu receipt import disabled");
        self.check_registration(authenticated)?;
        let receipt = authenticated.receipt().data();
        let limit = self
            .approved
            .get(&(receipt.mint_id.clone(), receipt.policy_version.clone()))
            .ok_or_else(|| anyhow::anyhow!("receipt mint policy not approved"))?;
        ensure!(
            limit.maximum_gross_sats > 0
                && limit.maximum_net_sats > 0
                && limit.maximum_gross_sats <= i64::MAX as u64
                && limit.maximum_net_sats <= limit.maximum_gross_sats,
            "invalid core receipt limit"
        );
        ensure!(
            receipt.gross_sats <= limit.maximum_gross_sats
                && receipt.net_sats <= limit.maximum_net_sats,
            "receipt exceeds core mint limit"
        );
        ensure!(
            remaining.receipt_count > 0
                && receipt.net_sats <= remaining.credit_sats
                && receipt.net_sats <= remaining.custody_sats,
            "core receipt/exposure budget exhausted"
        );
        Ok(())
    }

    pub(crate) fn check_registration(&self, authenticated: &PeerReceipt) -> Result<()> {
        let receipt = authenticated.receipt().data();
        // The authentication token cannot be reused under another registration.
        ensure!(
            authenticated.wallet_uid == self.wallet_uid
                && authenticated.core_epoch == self.core_epoch
                && receipt.wallet_instance == self.wallet_instance
                && receipt.wallet_epoch == self.wallet_epoch,
            "wallet registration changed"
        );
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn fixture_authenticate(policy: &ReceiptPolicy, bytes: &[u8]) -> Result<PeerReceipt> {
    policy.authenticate(&fixture_peer(policy), bytes)
}

#[cfg(test)]
pub(crate) fn fixture_peer(policy: &ReceiptPolicy) -> KernelPeer {
    KernelPeer {
        sender_uid: policy.wallet_uid,
        receiver_uid: policy.wallet_uid + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightning_goats_cashu_receipt::ReceiptData;
    fn fixture() -> (ReceiptPolicy, Vec<u8>, KernelPeer) {
        let instance = Uuid::new_v4();
        let epoch = Uuid::new_v4();
        let receipt = ReceiptData {
            schema_version: 1,
            wallet_instance: instance,
            wallet_epoch: epoch,
            acquisition_id: Uuid::new_v4(),
            mint_id: "fixture".into(),
            policy_version: "v1".into(),
            transport: "cashu".into(),
            source_ref: "request".into(),
            source_author: None,
            unit: "sat".into(),
            gross_sats: 8,
            receive_fee_sats: 1,
            net_sats: 7,
            acquisition_evidence_digest: "07".repeat(32),
            credit_pool: "herd".into(),
        };
        let policy = ReceiptPolicy {
            wallet_uid: 10000,
            wallet_instance: instance,
            wallet_epoch: epoch,
            core_epoch: Uuid::new_v4(),
            import_enabled: true,
            limits: ImportLimits {
                receipts_per_24h: 10,
                unlimited_receipts_per_24h: false,
                credit_sats_per_24h: 100,
                maximum_custody_sats: 100,
            },
            approved: BTreeMap::from([(
                ("fixture".into(), "v1".into()),
                MintImportLimit {
                    maximum_gross_sats: 8,
                    maximum_net_sats: 7,
                },
            )]),
        };
        (
            policy,
            WireReceipt::validate(receipt).unwrap().encode().unwrap(),
            KernelPeer {
                sender_uid: 10000,
                receiver_uid: 10001,
            },
        )
    }
    #[test]
    fn kernel_identity_precedes_parsing_and_registration_is_pinned() -> Result<()> {
        let (policy, bytes, peer) = fixture();
        assert!(policy.authenticate(&peer, &bytes).is_ok());
        for invalid in [
            KernelPeer {
                sender_uid: 0,
                receiver_uid: 10001,
            },
            KernelPeer {
                sender_uid: 10000,
                receiver_uid: 0,
            },
            KernelPeer {
                sender_uid: 10001,
                receiver_uid: 10001,
            },
            KernelPeer {
                sender_uid: 10000,
                receiver_uid: 10000,
            },
        ] {
            let error = policy
                .authenticate(&invalid, b"invalid JSON and secret marker")
                .err()
                .unwrap();
            assert_eq!(error.to_string(), "wallet peer not authorized");
        }
        let (other, _, _) = fixture();
        assert!(other.authenticate(&peer, &bytes).is_err());
        Ok(())
    }
    #[test]
    fn new_import_has_separate_limits_but_disabled_import_can_authenticate_replays() -> Result<()> {
        let (mut policy, bytes, peer) = fixture();
        let receipt = policy.authenticate(&peer, &bytes)?;
        let budget = RemainingBudget {
            receipt_count: 1,
            credit_sats: 7,
            custody_sats: 7,
        };
        policy.check_new(&receipt, &budget)?;
        assert!(
            policy
                .check_new(&receipt, &RemainingBudget::default())
                .is_err()
        );
        assert!(
            policy
                .check_new(
                    &receipt,
                    &RemainingBudget {
                        receipt_count: 1,
                        credit_sats: 6,
                        custody_sats: 7
                    }
                )
                .is_err()
        );
        assert!(
            policy
                .check_new(
                    &receipt,
                    &RemainingBudget {
                        receipt_count: 1,
                        credit_sats: 7,
                        custody_sats: 6
                    }
                )
                .is_err()
        );
        policy.import_enabled = false;
        assert!(policy.authenticate(&peer, &bytes).is_ok());
        assert!(policy.check_new(&receipt, &budget).is_err());
        policy.import_enabled = true;
        policy.wallet_uid = 10002;
        assert!(policy.check_new(&receipt, &budget).is_err());
        policy.wallet_uid = 10000;
        policy.approved.clear();
        assert!(policy.check_new(&receipt, &budget).is_err());
        Ok(())
    }
    #[tokio::test]
    async fn same_runtime_socket_cannot_claim_the_separate_wallet_identity() -> Result<()> {
        let (socket, _other) = UnixStream::pair()?;
        let peer = KernelPeer::from_socket(&socket)?;
        let (mut policy, bytes, _) = fixture();
        policy.wallet_uid = crate::private_ipc::uid()?;
        assert!(policy.authenticate(&peer, &bytes).is_err());
        Ok(())
    }
}
