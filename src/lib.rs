#![forbid(unsafe_code)]

mod bolt11;
#[cfg(unix)]
mod private_ipc;
mod sqlite;
#[cfg(unix)]
pub mod state_maintenance;

pub mod config;
pub mod core_bridge;
pub mod cyberherd;
pub mod domain;
pub mod feeder;
pub mod feeding_presentation;
pub mod gateway;
pub mod informational;
pub mod ledger;
pub mod lnurl;
pub mod messaging;
pub mod nostr;
pub mod nostr_event;
pub mod openhab;
pub mod overlay;
pub mod payout;
pub mod presentation;
pub mod public_network;
pub mod quotes;
pub mod reward_funding;
pub mod secrets;
pub mod strike;
pub mod zap;
pub mod zap_receipts;

pub mod http;

#[cfg(test)]
#[path = "../tests/support/invoices.rs"]
mod test_invoices;

pub mod server;
