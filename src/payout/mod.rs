//! Outgoing adapters. No receive-daemon credential or physical capability is
//! accepted here. Recipient discovery alone never authorizes a transfer.
pub mod funding;
pub mod invoice;
pub mod outcome;
pub mod provider;
pub mod recipient;
pub mod store;
