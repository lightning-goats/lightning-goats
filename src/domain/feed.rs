use std::{error::Error, fmt};

use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedLedger {
    threshold_sats: u64,
    credit_sats: u64,
}

impl FeedLedger {
    pub fn new(threshold_sats: u64) -> Result<Self, FeedError> {
        if threshold_sats == 0 {
            return Err(FeedError::ZeroThreshold);
        }
        Ok(Self {
            threshold_sats,
            credit_sats: 0,
        })
    }

    pub fn with_credit(threshold_sats: u64, credit_sats: u64) -> Result<Self, FeedError> {
        let mut ledger = Self::new(threshold_sats)?;
        ledger.credit_sats = credit_sats;
        Ok(ledger)
    }

    pub fn credit(&mut self, amount_sats: u64) -> Result<(), FeedError> {
        self.credit_sats = self
            .credit_sats
            .checked_add(amount_sats)
            .ok_or(FeedError::CreditOverflow)?;
        Ok(())
    }

    fn debit_confirmed_feed(&mut self) -> Result<(), FeedError> {
        if self.credit_sats < self.threshold_sats {
            return Err(FeedError::NoFeedDue);
        }
        self.credit_sats -= self.threshold_sats;
        Ok(())
    }

    #[must_use]
    pub const fn threshold_sats(&self) -> u64 {
        self.threshold_sats
    }

    #[must_use]
    pub const fn credit_sats(&self) -> u64 {
        self.credit_sats
    }

    #[must_use]
    pub const fn feeds_due(&self) -> u64 {
        self.credit_sats / self.threshold_sats
    }

    #[must_use]
    pub const fn remainder_sats(&self) -> u64 {
        self.credit_sats % self.threshold_sats
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedAttemptStatus {
    IntentCommitted,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedAttempt {
    pub id: Uuid,
    pub status: FeedAttemptStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedRuntime {
    ledger: FeedLedger,
    active_attempt: Option<FeedAttempt>,
}

impl FeedRuntime {
    pub fn new(ledger: FeedLedger) -> Self {
        Self {
            ledger,
            active_attempt: None,
        }
    }

    #[must_use]
    pub const fn ledger(&self) -> &FeedLedger {
        &self.ledger
    }

    #[must_use]
    pub const fn active_attempt(&self) -> Option<FeedAttempt> {
        self.active_attempt
    }

    pub fn credit(&mut self, amount_sats: u64) -> Result<(), FeedError> {
        self.ledger.credit(amount_sats)
    }

    pub fn begin_next_feed(&mut self, override_enabled: bool) -> Result<FeedAttempt, FeedError> {
        if override_enabled {
            return Err(FeedError::OverrideEnabled);
        }
        if self.active_attempt.is_some() {
            return Err(FeedError::AttemptAlreadyActive);
        }
        if self.ledger.feeds_due() == 0 {
            return Err(FeedError::NoFeedDue);
        }

        let attempt = FeedAttempt {
            id: Uuid::new_v4(),
            status: FeedAttemptStatus::IntentCommitted,
        };
        self.active_attempt = Some(attempt);
        Ok(attempt)
    }

    pub fn confirm_feed(&mut self, attempt_id: Uuid) -> Result<(), FeedError> {
        self.require_attempt(attempt_id, FeedAttemptStatus::IntentCommitted)?;
        self.ledger.debit_confirmed_feed()?;
        self.active_attempt = None;
        Ok(())
    }

    pub fn mark_feed_unknown(&mut self, attempt_id: Uuid) -> Result<(), FeedError> {
        self.require_attempt(attempt_id, FeedAttemptStatus::IntentCommitted)?;
        self.active_attempt = Some(FeedAttempt {
            id: attempt_id,
            status: FeedAttemptStatus::Unknown,
        });
        Ok(())
    }

    pub fn reconcile_unknown_as_fed(&mut self, attempt_id: Uuid) -> Result<(), FeedError> {
        self.require_attempt(attempt_id, FeedAttemptStatus::Unknown)?;
        self.ledger.debit_confirmed_feed()?;
        self.active_attempt = None;
        Ok(())
    }

    pub fn reconcile_unknown_as_not_fed(&mut self, attempt_id: Uuid) -> Result<(), FeedError> {
        self.require_attempt(attempt_id, FeedAttemptStatus::Unknown)?;
        self.active_attempt = None;
        Ok(())
    }

    fn require_attempt(
        &self,
        attempt_id: Uuid,
        expected_status: FeedAttemptStatus,
    ) -> Result<(), FeedError> {
        match self.active_attempt {
            Some(attempt) if attempt.id == attempt_id && attempt.status == expected_status => {
                Ok(())
            }
            Some(attempt) if attempt.id != attempt_id => Err(FeedError::AttemptIdMismatch),
            Some(_) => Err(FeedError::InvalidAttemptState),
            None => Err(FeedError::NoActiveAttempt),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedError {
    ZeroThreshold,
    CreditOverflow,
    NoFeedDue,
    OverrideEnabled,
    AttemptAlreadyActive,
    NoActiveAttempt,
    AttemptIdMismatch,
    InvalidAttemptState,
}

impl fmt::Display for FeedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::ZeroThreshold => "feeder threshold must be greater than zero",
            Self::CreditOverflow => "feed credit overflow",
            Self::NoFeedDue => "no feed is due",
            Self::OverrideEnabled => "automatic feeding is blocked by FeederOverride",
            Self::AttemptAlreadyActive => "a feed attempt is already active",
            Self::NoActiveAttempt => "no feed attempt is active",
            Self::AttemptIdMismatch => "feed attempt id does not match the active attempt",
            Self::InvalidAttemptState => "feed attempt is in an invalid state for this transition",
        };
        formatter.write_str(message)
    }
}

impl Error for FeedError {}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn two_thresholds_leave_correct_remainder() {
        let ledger = FeedLedger::with_credit(1_000, 2_340).unwrap();
        assert_eq!(ledger.feeds_due(), 2);
        assert_eq!(ledger.remainder_sats(), 340);
    }

    #[test]
    fn override_blocks_feed_without_losing_credit() {
        let ledger = FeedLedger::with_credit(1_000, 2_340).unwrap();
        let mut runtime = FeedRuntime::new(ledger);

        assert_eq!(
            runtime.begin_next_feed(true),
            Err(FeedError::OverrideEnabled)
        );
        assert_eq!(runtime.ledger().credit_sats(), 2_340);
        assert_eq!(runtime.ledger().feeds_due(), 2);
    }

    #[test]
    fn confirmed_feeds_debit_one_threshold_each() {
        let ledger = FeedLedger::with_credit(1_000, 2_340).unwrap();
        let mut runtime = FeedRuntime::new(ledger);

        let first = runtime.begin_next_feed(false).unwrap();
        assert_eq!(runtime.ledger().credit_sats(), 2_340);
        runtime.confirm_feed(first.id).unwrap();
        assert_eq!(runtime.ledger().credit_sats(), 1_340);

        let second = runtime.begin_next_feed(false).unwrap();
        runtime.confirm_feed(second.id).unwrap();
        assert_eq!(runtime.ledger().credit_sats(), 340);
        assert_eq!(runtime.ledger().feeds_due(), 0);
    }

    #[test]
    fn unknown_feed_does_not_debit_or_retry() {
        let ledger = FeedLedger::with_credit(1_000, 1_340).unwrap();
        let mut runtime = FeedRuntime::new(ledger);

        let attempt = runtime.begin_next_feed(false).unwrap();
        runtime.mark_feed_unknown(attempt.id).unwrap();

        assert_eq!(runtime.ledger().credit_sats(), 1_340);
        assert_eq!(
            runtime.begin_next_feed(false),
            Err(FeedError::AttemptAlreadyActive)
        );
    }

    #[test]
    fn unknown_confirmed_by_operator_debits_once() {
        let ledger = FeedLedger::with_credit(1_000, 1_340).unwrap();
        let mut runtime = FeedRuntime::new(ledger);
        let attempt = runtime.begin_next_feed(false).unwrap();
        runtime.mark_feed_unknown(attempt.id).unwrap();

        runtime.reconcile_unknown_as_fed(attempt.id).unwrap();
        assert_eq!(runtime.ledger().credit_sats(), 340);
        assert!(runtime.active_attempt().is_none());
    }

    #[test]
    fn unknown_rejected_by_operator_keeps_credit() {
        let ledger = FeedLedger::with_credit(1_000, 1_340).unwrap();
        let mut runtime = FeedRuntime::new(ledger);
        let attempt = runtime.begin_next_feed(false).unwrap();
        runtime.mark_feed_unknown(attempt.id).unwrap();

        runtime.reconcile_unknown_as_not_fed(attempt.id).unwrap();
        assert_eq!(runtime.ledger().credit_sats(), 1_340);
        assert!(runtime.active_attempt().is_none());
    }

    #[test]
    fn payments_can_arrive_while_backlog_is_draining() {
        let ledger = FeedLedger::with_credit(1_000, 2_340).unwrap();
        let mut runtime = FeedRuntime::new(ledger);

        let first = runtime.begin_next_feed(false).unwrap();
        runtime.confirm_feed(first.id).unwrap();
        runtime.credit(700).unwrap();
        assert_eq!(runtime.ledger().credit_sats(), 2_040);
        assert_eq!(runtime.ledger().feeds_due(), 2);

        let second = runtime.begin_next_feed(false).unwrap();
        runtime.confirm_feed(second.id).unwrap();
        let third = runtime.begin_next_feed(false).unwrap();
        runtime.confirm_feed(third.id).unwrap();

        assert_eq!(runtime.ledger().credit_sats(), 40);
    }

    proptest! {
        #[test]
        fn confirmed_feeds_never_debit_more_than_credit(
            threshold in 1u64..1_000_000,
            credit in 0u64..10_000_000_000,
        ) {
            let ledger = FeedLedger::with_credit(threshold, credit).unwrap();
            let expected_feeds = credit / threshold;
            let expected_remainder = credit % threshold;
            let mut runtime = FeedRuntime::new(ledger);

            let mut confirmed = 0u64;
            while runtime.ledger().feeds_due() > 0 {
                let attempt = runtime.begin_next_feed(false).unwrap();
                runtime.confirm_feed(attempt.id).unwrap();
                confirmed += 1;
            }

            prop_assert_eq!(confirmed, expected_feeds);
            prop_assert_eq!(runtime.ledger().credit_sats(), expected_remainder);
        }
    }
}
