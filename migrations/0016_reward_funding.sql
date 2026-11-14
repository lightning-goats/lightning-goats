-- Inactive until explicit reward-funding initialization. No old credit/debit,
-- event, invoice, UUID or signed message is rewritten or credited again.
CREATE TABLE reward_funding_state (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    bootstrap_through INTEGER NOT NULL CHECK(bootstrap_through>=0),
    last_ledger_id INTEGER NOT NULL CHECK(last_ledger_id>=0),
    initialized_at INTEGER NOT NULL DEFAULT(unixepoch())
);
CREATE TABLE reward_funding_lots (
    ledger_id INTEGER PRIMARY KEY REFERENCES ledger_entries(id),
    kind TEXT NOT NULL CHECK(kind IN ('strike_btc','unavailable')),
    original_sats INTEGER NOT NULL CHECK(typeof(original_sats)='integer' AND original_sats>0),
    remaining_sats INTEGER NOT NULL CHECK(typeof(remaining_sats)='integer' AND remaining_sats>=0 AND remaining_sats<=original_sats),
    proof_hash TEXT NOT NULL
);
CREATE INDEX reward_funding_remaining ON reward_funding_lots(ledger_id) WHERE remaining_sats>0;
CREATE TABLE feed_reward_funding (
    request_id TEXT PRIMARY KEY REFERENCES feed_attempts(id),
    debit_id INTEGER NOT NULL UNIQUE REFERENCES ledger_entries(id),
    total_sats INTEGER NOT NULL CHECK(typeof(total_sats)='integer' AND total_sats>0),
    strike_sats INTEGER NOT NULL CHECK(typeof(strike_sats)='integer' AND strike_sats>=0 AND strike_sats<=total_sats),
    proof_hash TEXT NOT NULL
);
CREATE TABLE feed_reward_funding_parts (
    request_id TEXT NOT NULL REFERENCES feed_reward_funding(request_id) DEFERRABLE INITIALLY DEFERRED,
    ledger_id INTEGER NOT NULL REFERENCES reward_funding_lots(ledger_id),
    sats INTEGER NOT NULL CHECK(typeof(sats)='integer' AND sats>0),
    PRIMARY KEY(request_id,ledger_id)
);
CREATE TABLE reward_reservations (
    id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL UNIQUE REFERENCES feed_reward_funding(request_id),
    epoch TEXT NOT NULL,
    owner TEXT NOT NULL,
    plan_hash TEXT NOT NULL,
    funding_hash TEXT NOT NULL,
    policy_json TEXT NOT NULL,
    reserved_sats INTEGER NOT NULL CHECK(typeof(reserved_sats)='integer' AND reserved_sats>0),
    created_at INTEGER NOT NULL DEFAULT(unixepoch())
);
CREATE INDEX reward_reservation_window ON reward_reservations(created_at,reserved_sats);

CREATE TRIGGER reward_funding_state_monotone BEFORE UPDATE ON reward_funding_state
WHEN NEW.singleton IS NOT OLD.singleton OR NEW.bootstrap_through IS NOT OLD.bootstrap_through
 OR NEW.initialized_at IS NOT OLD.initialized_at OR NEW.last_ledger_id<OLD.last_ledger_id
BEGIN SELECT RAISE(ABORT,'funding activation/cursor is immutable or monotone'); END;
CREATE TRIGGER reward_funding_state_retained BEFORE DELETE ON reward_funding_state
BEGIN SELECT RAISE(ABORT,'funding history is retained'); END;
CREATE TRIGGER reward_funding_lot_monotone BEFORE UPDATE ON reward_funding_lots
WHEN NEW.ledger_id IS NOT OLD.ledger_id OR NEW.kind IS NOT OLD.kind
 OR NEW.original_sats IS NOT OLD.original_sats OR NEW.proof_hash IS NOT OLD.proof_hash
 OR NEW.remaining_sats>OLD.remaining_sats
BEGIN SELECT RAISE(ABORT,'funding source is immutable; remaining funds cannot increase'); END;
CREATE TRIGGER reward_funding_lot_retained BEFORE DELETE ON reward_funding_lots
BEGIN SELECT RAISE(ABORT,'funding source is retained'); END;
CREATE TRIGGER feed_reward_funding_immutable BEFORE UPDATE ON feed_reward_funding
BEGIN SELECT RAISE(ABORT,'confirmed funding is immutable'); END;
CREATE TRIGGER feed_reward_funding_retained BEFORE DELETE ON feed_reward_funding
BEGIN SELECT RAISE(ABORT,'confirmed funding is retained'); END;
CREATE TRIGGER feed_reward_part_immutable BEFORE UPDATE ON feed_reward_funding_parts
BEGIN SELECT RAISE(ABORT,'confirmed funding part is immutable'); END;
CREATE TRIGGER feed_reward_part_retained BEFORE DELETE ON feed_reward_funding_parts
BEGIN SELECT RAISE(ABORT,'confirmed funding part is retained'); END;
CREATE TRIGGER reward_reservation_immutable BEFORE UPDATE ON reward_reservations
BEGIN SELECT RAISE(ABORT,'reward reservation is immutable'); END;
CREATE TRIGGER reward_reservation_retained BEFORE DELETE ON reward_reservations
BEGIN SELECT RAISE(ABORT,'reward reservation is retained'); END;
