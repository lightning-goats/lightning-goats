//! CyberHerd domain, isolated from provider settlement and physical actuation.
//!
//! Network adapters must authenticate events and settlement identities before
//! supplying domain commands. This module never mints feed credit or submits a
//! feeder/payout command. The current implementation progress is tracked in
//! docs/planning/cyberherd-implementation-plan.md.

pub mod calendar;
pub mod discovery;
pub mod ingress;
pub mod membership;
#[cfg(unix)]
pub mod payout_bridge;
pub mod relay;
#[cfg(unix)]
pub mod result_worker;
#[cfg(unix)]
pub mod reward_worker;
pub mod rewards;
#[cfg(unix)]
pub mod runtime;
pub mod shares;
pub mod social;
pub mod store;
pub mod targets;
pub mod templates;
pub mod worker;

#[cfg(all(test, unix))]
mod ingress_tests;

#[cfg(all(test, unix))]
mod rewards_tests;
