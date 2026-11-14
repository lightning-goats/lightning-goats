-- Partial reports retain the whole reservation. Only the final hash/count and
-- gross-total check may release it. No payment/feeding ledger row is changed.
CREATE TABLE reward_budget_clock (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    observed_at INTEGER NOT NULL CHECK(observed_at>=0)
);
INSERT INTO reward_budget_clock VALUES(1,0);
CREATE TABLE reward_outcome_manifests (
    reservation_id TEXT PRIMARY KEY REFERENCES reward_reservations(id),
    header_json TEXT NOT NULL
);
CREATE TABLE reward_outcome_rows (
    reservation_id TEXT NOT NULL REFERENCES reward_outcome_manifests(reservation_id),
    position INTEGER NOT NULL CHECK(position>=0 AND position<1000),
    member TEXT NOT NULL,
    row_json TEXT NOT NULL,
    before_hash TEXT NOT NULL,
    chain_hash TEXT NOT NULL,
    gross_sats INTEGER NOT NULL CHECK(typeof(gross_sats)='integer' AND gross_sats>0),
    spent_sats INTEGER NOT NULL CHECK(typeof(spent_sats)='integer' AND spent_sats>=0),
    held_sats INTEGER NOT NULL CHECK(typeof(held_sats)='integer' AND held_sats>=0),
    verified_at INTEGER NOT NULL CHECK(verified_at>=0),
    payment_id TEXT UNIQUE,
    invoice_hash TEXT UNIQUE,
    PRIMARY KEY(reservation_id,position),
    UNIQUE(reservation_id,member),
    CHECK(spent_sats+held_sats<=gross_sats)
);
CREATE TABLE reward_outcome_finals (
    reservation_id TEXT PRIMARY KEY REFERENCES reward_outcome_manifests(reservation_id),
    progress_json TEXT NOT NULL,
    held_sats INTEGER NOT NULL CHECK(typeof(held_sats)='integer' AND held_sats>=0)
);
CREATE TRIGGER reward_budget_clock_monotone BEFORE UPDATE ON reward_budget_clock
WHEN NEW.singleton IS NOT OLD.singleton OR NEW.observed_at<OLD.observed_at
BEGIN SELECT RAISE(ABORT,'reward budget clock is monotone'); END;
CREATE TRIGGER reward_budget_clock_retained BEFORE DELETE ON reward_budget_clock
BEGIN SELECT RAISE(ABORT,'reward budget clock is retained'); END;
CREATE TRIGGER reward_manifest_immutable BEFORE UPDATE ON reward_outcome_manifests
BEGIN SELECT RAISE(ABORT,'reward manifest is immutable'); END;
CREATE TRIGGER reward_manifest_retained BEFORE DELETE ON reward_outcome_manifests
BEGIN SELECT RAISE(ABORT,'reward manifest is retained'); END;
CREATE TRIGGER reward_outcome_row_immutable BEFORE UPDATE ON reward_outcome_rows
BEGIN SELECT RAISE(ABORT,'verified outcome is immutable'); END;
CREATE TRIGGER reward_outcome_row_retained BEFORE DELETE ON reward_outcome_rows
BEGIN SELECT RAISE(ABORT,'verified outcome is retained'); END;
CREATE TRIGGER reward_outcome_final_immutable BEFORE UPDATE ON reward_outcome_finals
BEGIN SELECT RAISE(ABORT,'final outcome is immutable'); END;
CREATE TRIGGER reward_outcome_final_retained BEFORE DELETE ON reward_outcome_finals
BEGIN SELECT RAISE(ABORT,'final outcome is retained'); END;
