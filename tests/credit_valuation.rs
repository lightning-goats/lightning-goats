//! Offline arithmetic/contract tests only: no wallet, oracle, funds or feeder.
use lightning_goats::domain::credit::{
    Asset, AssetAmount, CreditTerms, MAX_ACCOUNTING_UNITS, PICONERO_PER_XMR, SATS_PER_BTC,
    XmrBtcRate,
};

fn amount(asset: Asset, atomic: u64) -> AssetAmount {
    AssetAmount::new(asset, atomic).unwrap()
}

fn xmr(atomic: u64) -> AssetAmount {
    amount(Asset::Xmr, atomic)
}

fn rate(numerator: u64, denominator: u64) -> XmrBtcRate {
    XmrBtcRate::new(numerator, denominator, "synthetic-test", 100).unwrap()
}

#[test]
fn units_are_asset_tagged_and_xmr_is_not_native_sats() {
    assert_eq!(Asset::Btc.atomic_units_per_coin(), SATS_PER_BTC);
    assert_eq!(Asset::Xmr.atomic_units_per_coin(), PICONERO_PER_XMR);
    let btc = amount(Asset::Btc, 425);
    assert_eq!(btc.asset(), Asset::Btc);
    assert_eq!(btc.atomic_units(), 425);
    assert_eq!(btc.native_sats().unwrap(), 425);
    assert!(xmr(425).native_sats().is_err());
}

#[test]
fn storage_bounds_and_zero_cumulative_amounts_are_explicit() {
    for asset in [Asset::Btc, Asset::Xmr] {
        assert!(AssetAmount::new(asset, 0).is_ok());
        assert!(AssetAmount::new(asset, MAX_ACCOUNTING_UNITS).is_ok());
        assert!(AssetAmount::new(asset, MAX_ACCOUNTING_UNITS + 1).is_err());
        assert!(AssetAmount::new(asset, u64::MAX).is_err());
    }
}

#[test]
fn btc_identity_credit_needs_no_oracle() {
    let terms = CreditTerms::native_btc();
    for sats in [0, 1, 425, 1000, MAX_ACCOUNTING_UNITS] {
        assert_eq!(
            terms.cumulative_sats(amount(Asset::Btc, sats)).unwrap(),
            sats
        );
    }
    let update = terms
        .assess_update(amount(Asset::Btc, 300), 300, amount(Asset::Btc, 475))
        .unwrap();
    assert_eq!(update.delta_sats, 175);
}

#[test]
fn zero_or_out_of_range_quote_terms_are_rejected() {
    for (atomic, sats) in [
        (0, 1),
        (1, 0),
        (MAX_ACCOUNTING_UNITS + 1, 1),
        (1, MAX_ACCOUNTING_UNITS + 1),
    ] {
        assert!(CreditTerms::quoted_xmr(atomic, sats).is_err());
    }
}

#[test]
fn valuation_rejects_asset_mismatches() {
    assert!(CreditTerms::native_btc().cumulative_sats(xmr(1)).is_err());
    let terms = CreditTerms::quoted_xmr(3, 2).unwrap();
    assert!(terms.cumulative_sats(amount(Asset::Btc, 3)).is_err());
    assert!(
        terms
            .assess_update(amount(Asset::Btc, 0), 0, xmr(3))
            .is_err()
    );
    assert!(
        terms
            .assess_update(xmr(0), 0, amount(Asset::Btc, 3))
            .is_err()
    );
}

#[test]
fn partials_credit_cumulatively_without_per_receipt_rounding_loss() {
    let terms = CreditTerms::quoted_xmr(7, 1000).unwrap();
    let mut credited = 0;
    for atomic in 1..=7 {
        let update = terms
            .assess_update(xmr(atomic - 1), credited, xmr(atomic))
            .unwrap();
        credited += update.delta_sats;
        assert_eq!(credited, update.cumulative_credit_sats);
        assert_eq!(update.cumulative_eligible, xmr(atomic));
    }
    assert_eq!(credited, 1000);
    assert_eq!(credited, terms.cumulative_sats(xmr(7)).unwrap());
}

#[test]
fn dust_advances_atomic_watermark_even_without_a_sat_grant() {
    let terms = CreditTerms::quoted_xmr(3, 1).unwrap();
    let first = terms.assess_update(xmr(0), 0, xmr(1)).unwrap();
    assert_eq!(first.delta_sats, 0);
    assert_eq!(first.cumulative_eligible.atomic_units(), 1);
    let second = terms
        .assess_update(
            first.cumulative_eligible,
            first.cumulative_credit_sats,
            xmr(2),
        )
        .unwrap();
    assert_eq!(second.delta_sats, 0);
    let final_update = terms
        .assess_update(
            second.cumulative_eligible,
            second.cumulative_credit_sats,
            xmr(3),
        )
        .unwrap();
    assert_eq!(final_update.delta_sats, 1);
}

#[test]
fn duplicate_cumulative_observation_has_zero_delta() {
    let terms = CreditTerms::quoted_xmr(7, 1000).unwrap();
    let first = terms.assess_update(xmr(0), 0, xmr(4)).unwrap();
    let duplicate = terms
        .assess_update(
            first.cumulative_eligible,
            first.cumulative_credit_sats,
            xmr(4),
        )
        .unwrap();
    assert_eq!(duplicate.delta_sats, 0);
}

#[test]
fn regressed_atomic_total_is_rejected_even_if_both_round_to_zero() {
    let terms = CreditTerms::quoted_xmr(100, 1).unwrap();
    assert!(terms.assess_update(xmr(2), 0, xmr(1)).is_err());
    let terms = CreditTerms::quoted_xmr(100, 1000).unwrap();
    assert!(terms.assess_update(xmr(100), 1000, xmr(50)).is_err());
    // After a rejected regression, retained 100/1000 state yields no new grant.
    assert_eq!(
        terms
            .assess_update(xmr(100), 1000, xmr(100))
            .unwrap()
            .delta_sats,
        0
    );
}

#[test]
fn inconsistent_prior_credit_is_not_silently_repaired() {
    let terms = CreditTerms::quoted_xmr(7, 1000).unwrap();
    assert!(terms.assess_update(xmr(7), 999, xmr(7)).is_err());
    assert!(terms.assess_update(xmr(7), 1001, xmr(8)).is_err());
    assert!(terms.assess_update(xmr(0), 1, xmr(7)).is_err());
}

#[test]
fn cumulative_valuation_handles_large_integer_values_exactly() {
    let terms = CreditTerms::quoted_xmr(MAX_ACCOUNTING_UNITS, MAX_ACCOUNTING_UNITS).unwrap();
    assert_eq!(
        terms.cumulative_sats(xmr(MAX_ACCOUNTING_UNITS)).unwrap(),
        MAX_ACCOUNTING_UNITS
    );
    let too_much_credit = CreditTerms::quoted_xmr(1, MAX_ACCOUNTING_UNITS).unwrap();
    assert!(too_much_credit.cumulative_sats(xmr(2)).is_err());
}

#[test]
fn timely_overpayment_uses_the_agreed_ratio() {
    let terms = CreditTerms::quoted_xmr(7, 1000).unwrap();
    assert_eq!(terms.cumulative_sats(xmr(14)).unwrap(), 2000);
    assert_eq!(
        terms
            .assess_update(xmr(7), 1000, xmr(14))
            .unwrap()
            .delta_sats,
        1000
    );
}

#[test]
fn quote_rounds_requested_piconero_up_and_full_payment_matches_target() {
    // A synthetic example rate, not a current market price.
    let quote = rate(300_000, 1).quote(1000, 105, 405, 10).unwrap();
    assert_eq!(quote.terms().expected_atomic(), 3_333_333_334);
    assert_eq!(quote.terms().asset(), Asset::Xmr);
    assert_eq!(quote.terms().target_sats(), 1000);
    assert_eq!(
        quote.terms().cumulative_sats(xmr(3_333_333_334)).unwrap(),
        1000
    );
    assert_eq!(
        quote.terms().cumulative_sats(xmr(3_333_333_333)).unwrap(),
        999
    );
    assert_eq!(quote.rate().source(), "synthetic-test");
    assert_eq!(quote.rate().ratio(), (300_000, 1));
    assert_eq!(quote.rate().observed_at(), 100);
    assert_eq!(quote.issued_at(), 105);
    assert_eq!(quote.expires_at(), 405);
}

#[test]
fn rational_rate_direction_and_exact_ceil_are_correct() {
    let quote = rate(3, 2).quote(1, 100, 200, 1).unwrap();
    assert_eq!(quote.terms().expected_atomic(), 666_666_666_667);
    let exact = rate(500_000, 1).quote(1000, 100, 200, 1).unwrap();
    assert_eq!(exact.terms().expected_atomic(), 2_000_000_000);
}

#[test]
fn no_rate_update_revalues_an_existing_quote() {
    let original = rate(500_000, 1).quote(1000, 100, 200, 1).unwrap();
    let later = rate(1_000_000, 1).quote(1000, 100, 200, 1).unwrap();
    assert_ne!(
        original.terms().expected_atomic(),
        later.terms().expected_atomic()
    );
    assert_eq!(original.terms().expected_atomic(), 2_000_000_000);
    assert_eq!(
        original
            .terms()
            .cumulative_sats(xmr(2_000_000_000))
            .unwrap(),
        1000
    );
}

#[test]
fn stale_future_and_malformed_rate_evidence_is_rejected() {
    assert!(rate(1, 1).quote(1, 111, 200, 10).is_err());
    assert!(rate(1, 1).quote(1, 99, 200, 10).is_err());
    assert!(rate(1, 1).quote(1, 110, 200, 10).is_ok());
    assert!(XmrBtcRate::new(0, 1, "test", 100).is_err());
    assert!(XmrBtcRate::new(1, 0, "test", 100).is_err());
    assert!(XmrBtcRate::new(1, 1, "test", -1).is_err());
    for source in [
        "",
        "space source",
        "bad\nsource",
        "https://unexpected/?secret=x",
    ] {
        assert!(XmrBtcRate::new(1, 1, source, 100).is_err());
    }
    assert!(XmrBtcRate::new(1, 1, &"x".repeat(97), 100).is_err());
}

#[test]
fn invalid_quote_policy_and_checked_overflow_fail_closed() {
    let rate = rate(1, 1);
    for (target, issued, expires, age) in [
        (0, 100, 200, 10),
        (MAX_ACCOUNTING_UNITS + 1, 100, 200, 10),
        (1, -1, 200, 10),
        (1, 100, 100, 10),
        (1, 100, 99, 10),
        (1, 100, 200, 0),
    ] {
        assert!(rate.quote(target, issued, expires, age).is_err());
    }
    assert!(rate.quote(MAX_ACCOUNTING_UNITS, 100, 200, 10).is_err());
    assert!(
        XmrBtcRate::new(1, u64::MAX, "test", 100)
            .unwrap()
            .quote(MAX_ACCOUNTING_UNITS, 100, 200, 10)
            .is_err()
    );
}

#[test]
fn extreme_valid_timestamp_differences_do_not_overflow() {
    let quote = XmrBtcRate::new(1, 1, "test", 0)
        .unwrap()
        .quote(1, i64::MAX - 1, i64::MAX, u64::MAX)
        .unwrap();
    assert!(quote.accepts_first_seen_at(i64::MAX - 1));
    assert!(!quote.accepts_first_seen_at(i64::MAX));
}

#[test]
fn first_seen_window_does_not_depend_on_later_unlock_time() {
    let quote = rate(500_000, 1).quote(1000, 100, 200, 1).unwrap();
    assert!(!quote.accepts_first_seen_at(99));
    assert!(quote.accepts_first_seen_at(100));
    assert!(quote.accepts_first_seen_at(199));
    assert!(!quote.accepts_first_seen_at(200));
    assert!(!quote.accepts_first_seen_at(500));
    // A caller can evaluate at unlock time 500 using a persisted first-seen 199.
    // This module does not assert that unlock or authoritative observation occurred.
    assert!(quote.accepts_first_seen_at(199));
}

#[test]
fn partitioning_and_repeating_totals_does_not_change_final_credit() {
    for expected in 1..=32 {
        for target in [1, 7, 999, 1000, 2340] {
            let terms = CreditTerms::quoted_xmr(expected, target).unwrap();
            let mut credited = 0;
            for current in 1..=expected * 2 {
                let update = terms
                    .assess_update(xmr(current - 1), credited, xmr(current))
                    .unwrap();
                credited += update.delta_sats;
                assert_eq!(
                    terms
                        .assess_update(xmr(current), credited, xmr(current))
                        .unwrap()
                        .delta_sats,
                    0
                );
            }
            assert_eq!(credited, target * 2);
            assert_eq!(credited, terms.cumulative_sats(xmr(expected * 2)).unwrap());
        }
    }
}

#[test]
fn native_and_quoted_credit_share_one_unit_without_a_wallet_balance() {
    let native = CreditTerms::native_btc();
    let quoted = CreditTerms::quoted_xmr(100, 1000).unwrap();
    let total = native.cumulative_sats(amount(Asset::Btc, 300)).unwrap()
        + quoted.cumulative_sats(xmr(100)).unwrap()
        + native.cumulative_sats(amount(Asset::Btc, 40)).unwrap()
        + quoted.cumulative_sats(xmr(100)).unwrap();
    assert_eq!(total, 2340);
    assert_eq!(total / 1000, 2);
    assert_eq!(total % 1000, 340);
    // Arithmetic only. This does not claim an actual ledger debit or feeder test.
}
