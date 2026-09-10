#![forbid(unsafe_code)]

mod bolt11;

pub mod config;
pub mod domain;
pub mod feeder;
pub mod gateway;
pub mod informational;
pub mod ledger;
pub mod lnurl;
pub mod messaging;
pub mod nostr;
pub mod openhab;
pub mod overlay;
pub mod presentation;
pub mod secrets;
pub mod strike;

pub mod http;

#[cfg(test)]
#[path = "../tests/support/invoices.rs"]
mod test_invoices;

pub mod server;
