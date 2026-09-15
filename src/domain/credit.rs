//! Asset-tagged amounts and immutable sats-credit valuation primitives.
//!
//! This module does not observe a wallet, establish settlement, or grant credit.
//! An adapter must authenticate receipts, apply finality/quote-time policy, and
//! deduplicate them before passing cumulative eligible amounts here. The ledger
//! must read the prior allocation and persist the new allocation, credit delta
//! and public event in ONE serialized transaction. A calculated delta is not an
//! idempotency key or an independently spendable balance.

use anyhow::{Result, bail};

/// Matches the nonnegative SQLite INTEGER range used by project accounting.
/// This is a storage bound, not an operational donation/quote limit.
pub const MAX_ACCOUNTING_UNITS: u64 = i64::MAX as u64;
pub const PICONERO_PER_XMR: u64 = 1_000_000_000_000;
pub const SATS_PER_BTC: u64 = 100_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asset {
    Btc,
    Xmr,
}

impl Asset {
    pub fn atomic_units_per_coin(self) -> u64 {
        match self {
            Self::Btc => SATS_PER_BTC,
            Self::Xmr => PICONERO_PER_XMR,
        }
    }
}

/// Private fields prevent bypassing the storage bound. Zero is valid for a
/// cumulative starting balance; individual receipt adapters must reject zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetAmount {
    asset: Asset,
    atomic_units: u64,
}

impl AssetAmount {
    pub fn new(asset: Asset, atomic_units: u64) -> Result<Self> {
        if atomic_units > MAX_ACCOUNTING_UNITS {
            bail!("asset amount exceeds accounting storage range");
        }
        Ok(Self {
            asset,
            atomic_units,
        })
    }

    pub fn asset(self) -> Asset {
        self.asset
    }

    pub fn atomic_units(self) -> u64 {
        self.atomic_units
    }

    /// BTC satoshis have identity valuation. XMR is never pretend BTC/msat.
    pub fn native_sats(self) -> Result<u64> {
        if self.asset != Asset::Btc {
            bail!("non-BTC amount requires an explicit credit valuation");
        }
        Ok(self.atomic_units)
    }
}

/// The agreed ratio, NOT a mutable spot price. For a rounded XMR invoice,
/// paying exactly expected_atomic always grants exactly target_sats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreditTerms {
    asset: Asset,
    expected_atomic: u64,
    target_sats: u64,
}

impl CreditTerms {
    pub fn native_btc() -> Self {
        Self {
            asset: Asset::Btc,
            expected_atomic: 1,
            target_sats: 1,
        }
    }

    /// Reconstruct persisted XMR terms only after validating their immutable
    /// quote/intent binding. This constructor does not authenticate a quote.
    pub fn quoted_xmr(expected_atomic: u64, target_sats: u64) -> Result<Self> {
        if expected_atomic == 0 || target_sats == 0 {
            bail!("quote amount and target credit must be positive");
        }
        if expected_atomic > MAX_ACCOUNTING_UNITS || target_sats > MAX_ACCOUNTING_UNITS {
            bail!("quote exceeds accounting storage range");
        }
        Ok(Self {
            asset: Asset::Xmr,
            expected_atomic,
            target_sats,
        })
    }

    pub fn asset(self) -> Asset {
        self.asset
    }

    pub fn expected_atomic(self) -> u64 {
        self.expected_atomic
    }

    pub fn target_sats(self) -> u64 {
        self.target_sats
    }

    pub fn cumulative_sats(self, eligible: AssetAmount) -> Result<u64> {
        if eligible.asset != self.asset {
            bail!("receipt asset does not match credit terms");
        }
        let product = u128::from(eligible.atomic_units)
            .checked_mul(u128::from(self.target_sats))
            .ok_or_else(|| anyhow::anyhow!("credit multiplication overflow"))?;
        let total = product / u128::from(self.expected_atomic);
        if total > u128::from(MAX_ACCOUNTING_UNITS) {
            bail!("valued credit exceeds accounting storage range");
        }
        Ok(total as u64)
    }

    /// Use cumulative eligible receipts, not callback totals or wallet balance.
    /// Regressions and inconsistent saved credit fail closed, never saturate.
    /// Dust must still persist the new atomic watermark even when delta is zero.
    pub fn assess_update(
        self,
        previous_eligible: AssetAmount,
        previously_credited_sats: u64,
        current_eligible: AssetAmount,
    ) -> Result<CreditUpdate> {
        let previous_total = self.cumulative_sats(previous_eligible)?;
        let current_total = self.cumulative_sats(current_eligible)?;
        if previous_total != previously_credited_sats {
            bail!("saved credit is inconsistent with the immutable valuation");
        }
        if current_eligible.atomic_units < previous_eligible.atomic_units {
            bail!("eligible receipt total regressed; reconciliation is required");
        }
        let delta_sats = current_total
            .checked_sub(previous_total)
            .ok_or_else(|| anyhow::anyhow!("valued credit regressed"))?;
        Ok(CreditUpdate {
            cumulative_eligible: current_eligible,
            cumulative_credit_sats: current_total,
            delta_sats,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreditUpdate {
    pub cumulative_eligible: AssetAmount,
    pub cumulative_credit_sats: u64,
    pub delta_sats: u64,
}

/// Immutable rate evidence, explicitly numerator/denominator SATS PER XMR.
/// No floating-point conversion or live oracle/network call is performed here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmrBtcRate {
    sats_per_xmr_numerator: u64,
    sats_per_xmr_denominator: u64,
    source: String,
    observed_at: i64,
}

impl XmrBtcRate {
    pub fn new(
        sats_per_xmr_numerator: u64,
        sats_per_xmr_denominator: u64,
        source: &str,
        observed_at: i64,
    ) -> Result<Self> {
        if sats_per_xmr_numerator == 0 || sats_per_xmr_denominator == 0 {
            bail!("exchange rate must be positive");
        }
        if source.is_empty()
            || source.len() > 96
            || !source
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
        {
            bail!("rate source must be a bounded source identifier");
        }
        if observed_at < 0 {
            bail!("rate observation time must be nonnegative");
        }
        Ok(Self {
            sats_per_xmr_numerator,
            sats_per_xmr_denominator,
            source: source.to_owned(),
            observed_at,
        })
    }

    pub fn ratio(&self) -> (u64, u64) {
        (self.sats_per_xmr_numerator, self.sats_per_xmr_denominator)
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn observed_at(&self) -> i64 {
        self.observed_at
    }

    /// Lock a sats-targeted quote. Caller supplies policy times and rate-age
    /// limit; there is deliberately no guessed production lifetime or source.
    pub fn quote(
        &self,
        target_sats: u64,
        issued_at: i64,
        expires_at: i64,
        max_rate_age_seconds: u64,
    ) -> Result<XmrQuote> {
        if target_sats == 0 || target_sats > MAX_ACCOUNTING_UNITS {
            bail!("quote target must be positive and within accounting range");
        }
        if issued_at < 0 || expires_at <= issued_at || max_rate_age_seconds == 0 {
            bail!("invalid quote lifetime or rate freshness policy");
        }
        if self.observed_at > issued_at {
            bail!("rate observation is in the future");
        }
        // Both timestamps are now nonnegative and ordered, so subtraction fits.
        if (issued_at - self.observed_at) as u64 > max_rate_age_seconds {
            bail!("exchange rate is stale");
        }
        let numerator = u128::from(target_sats)
            .checked_mul(u128::from(PICONERO_PER_XMR))
            .and_then(|n| n.checked_mul(u128::from(self.sats_per_xmr_denominator)))
            .ok_or_else(|| anyhow::anyhow!("quote calculation overflow"))?;
        let denominator = u128::from(self.sats_per_xmr_numerator);
        // Ceil without the potentially overflowing (numerator + denominator - 1).
        let required = numerator / denominator + u128::from(numerator % denominator != 0);
        if required > u128::from(MAX_ACCOUNTING_UNITS) {
            bail!("quoted XMR amount exceeds accounting storage range");
        }
        let terms = CreditTerms::quoted_xmr(required as u64, target_sats)?;
        Ok(XmrQuote {
            terms,
            rate: self.clone(),
            issued_at,
            expires_at,
        })
    }
}

/// A value object, not a payable intent. Persistence must additionally bind a
/// unique quote ID, network, recipient, subaddress and immutable policy version.
/// Deliberately no unchecked deserialization or public setter is provided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmrQuote {
    terms: CreditTerms,
    rate: XmrBtcRate,
    issued_at: i64,
    expires_at: i64,
}

impl XmrQuote {
    pub fn terms(&self) -> CreditTerms {
        self.terms
    }

    pub fn rate(&self) -> &XmrBtcRate {
        &self.rate
    }

    pub fn issued_at(&self) -> i64 {
        self.issued_at
    }

    pub fn expires_at(&self) -> i64 {
        self.expires_at
    }

    /// Apply to trusted, durable first observation of EACH receipt, not client,
    /// callback or block time. Unlock after expiry does not invalidate an earlier
    /// eligible observation. This check alone does not establish settlement.
    pub fn accepts_first_seen_at(&self, first_seen_at: i64) -> bool {
        first_seen_at >= self.issued_at && first_seen_at < self.expires_at
    }
}
