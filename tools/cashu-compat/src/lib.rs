//! Isolated Cashu wallet boundary; execution requires explicit protected policy.
#![forbid(unsafe_code)]

pub mod descriptor;
pub mod intake;
pub mod journal;
pub mod melt_derivation;
pub mod melt_reconciliation;
pub mod mint_policy;
pub mod operation_gate;
pub mod ownership;
pub mod transport;
pub mod wallet;
pub mod writer_lock;

#[cfg(test)]
pub(crate) fn test_directory() -> std::io::Result<tempfile::TempDir> {
    tempfile::Builder::new()
        .permissions(<std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700))
        .tempdir()
}
pub mod acquisition;
pub mod receipt_wire;

pub mod treasury;
pub mod treasury_recovery;

pub mod treasury_execution;

pub mod ingress;

// Reuse the core's protected Unix transport checks without its application graph.
#[path = "../../../src/private_ipc.rs"]
pub mod private_ipc;

pub mod service_config;

pub mod service_keys;

pub mod service;

pub mod receipt_delivery;
pub mod receipt_outbox;
pub mod relay;
pub mod scheduler;
pub mod status;

pub mod relay_intake;

pub mod publication;

pub mod publication_broker;

pub mod publication_delivery;
pub mod publication_nak;
pub mod publication_relay;
pub mod publication_signing;
pub mod signing_service;

pub mod treasury_ingress;

pub mod treasury_config;

pub mod treasury_scheduler;

pub mod treasury_split;

pub mod operator_reconciliation;
