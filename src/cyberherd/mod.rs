//! CyberHerd domain, isolated from provider settlement and physical actuation.
//!
//! Network adapters must authenticate events and settlement identities before
//! supplying domain commands. This module never mints feed credit or submits a
//! feeder/payout command. The current implementation progress is tracked in
//! docs/planning/cyberherd-implementation-plan.md.

pub mod membership;
pub mod shares;
pub mod store;
pub mod templates;
