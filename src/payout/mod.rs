//! Outgoing adapters. No receive-daemon credential or physical capability is
//! accepted here. Recipient discovery alone never authorizes a transfer.
pub mod cost_recovery;
pub mod execution;
pub mod funding;
pub mod inclusive_fees;
pub mod invoice;
pub mod outcome;
pub mod provider;
pub mod recipient;
pub mod results;
#[cfg(unix)]
pub mod runtime;
#[cfg(unix)]
pub mod signer;
pub mod store;
#[cfg(unix)]
pub mod worker;
