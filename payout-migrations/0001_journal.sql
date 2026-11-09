-- Separate payout identity only. Never apply to the core or membership database.
CREATE TABLE payout_binding (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    scope_json TEXT NOT NULL
);
CREATE TABLE payout_clock (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    observed_at INTEGER NOT NULL CHECK(observed_at>=0)
);
INSERT INTO payout_clock VALUES(1,0);
CREATE TABLE payout_grants (
    id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL UNIQUE,
    plan_hash TEXT NOT NULL UNIQUE,
    plan_json TEXT NOT NULL,
    qualification_json TEXT NOT NULL,
    reserved_sats INTEGER NOT NULL CHECK(reserved_sats>0),
    admitted_at INTEGER NOT NULL CHECK(admitted_at>=0)
);
CREATE TABLE payout_sources (
    receive_id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL,
    received_sats INTEGER NOT NULL CHECK(received_sats>0),
    consumed_sats INTEGER NOT NULL CHECK(consumed_sats>0 AND consumed_sats<=received_sats)
);
CREATE TABLE payout_source_parts (
    grant_id TEXT NOT NULL REFERENCES payout_grants(id),
    ledger_id INTEGER NOT NULL CHECK(ledger_id>0),
    receive_id TEXT NOT NULL REFERENCES payout_sources(receive_id),
    consumed_sats INTEGER NOT NULL CHECK(consumed_sats>0),
    PRIMARY KEY(grant_id,ledger_id),
    UNIQUE(grant_id,receive_id)
);
CREATE TABLE payout_rows (
    grant_id TEXT NOT NULL REFERENCES payout_grants(id),
    member TEXT NOT NULL,
    address TEXT NOT NULL,
    recipient_json TEXT NOT NULL,
    net_sats INTEGER NOT NULL CHECK(net_sats>0),
    fee_limit_sats INTEGER NOT NULL CHECK(fee_limit_sats>=0),
    gross_sats INTEGER NOT NULL CHECK(gross_sats=net_sats+fee_limit_sats),
    PRIMARY KEY(grant_id,member)
);
CREATE TABLE payout_journal (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    grant_id TEXT NOT NULL REFERENCES payout_grants(id),
    stage TEXT NOT NULL CHECK(stage='grant_admitted'),
    created_at INTEGER NOT NULL CHECK(created_at>=0),
    UNIQUE(grant_id,stage)
);
CREATE TRIGGER immutable_payout_binding BEFORE UPDATE ON payout_binding
BEGIN SELECT RAISE(ABORT,'payout scope requires explicit recovery'); END;
CREATE TRIGGER retained_payout_binding BEFORE DELETE ON payout_binding
BEGIN SELECT RAISE(ABORT,'payout scope is retained'); END;
CREATE TRIGGER monotonic_payout_clock BEFORE UPDATE ON payout_clock
WHEN NEW.singleton IS NOT OLD.singleton OR NEW.observed_at<OLD.observed_at
BEGIN SELECT RAISE(ABORT,'payout clock cannot move backwards'); END;
CREATE TRIGGER retained_payout_clock BEFORE DELETE ON payout_clock
BEGIN SELECT RAISE(ABORT,'payout clock is retained'); END;
CREATE TRIGGER monotonic_payout_sources BEFORE UPDATE ON payout_sources
WHEN NEW.receive_id IS NOT OLD.receive_id OR NEW.request_id IS NOT OLD.request_id
 OR NEW.received_sats IS NOT OLD.received_sats OR NEW.consumed_sats<=OLD.consumed_sats
BEGIN SELECT RAISE(ABORT,'payout source identity is immutable and consumption increases'); END;
CREATE TRIGGER retained_payout_sources BEFORE DELETE ON payout_sources
BEGIN SELECT RAISE(ABORT,'payout source consumption is retained'); END;
CREATE TRIGGER immutable_payout_grants BEFORE UPDATE ON payout_grants
BEGIN SELECT RAISE(ABORT,'payout grant is frozen'); END;
CREATE TRIGGER retained_payout_grants BEFORE DELETE ON payout_grants
BEGIN SELECT RAISE(ABORT,'payout grants are retained'); END;
CREATE TRIGGER immutable_payout_parts BEFORE UPDATE ON payout_source_parts
BEGIN SELECT RAISE(ABORT,'payout funding parts are frozen'); END;
CREATE TRIGGER retained_payout_parts BEFORE DELETE ON payout_source_parts
BEGIN SELECT RAISE(ABORT,'payout funding parts are retained'); END;
CREATE TRIGGER immutable_payout_rows BEFORE UPDATE ON payout_rows
BEGIN SELECT RAISE(ABORT,'payout destination and amounts are frozen'); END;
CREATE TRIGGER retained_payout_rows BEFORE DELETE ON payout_rows
BEGIN SELECT RAISE(ABORT,'payout rows are retained'); END;
CREATE TRIGGER immutable_payout_journal BEFORE UPDATE ON payout_journal
BEGIN SELECT RAISE(ABORT,'payout journal is append-only'); END;
CREATE TRIGGER retained_payout_journal BEFORE DELETE ON payout_journal
BEGIN SELECT RAISE(ABORT,'payout journal is retained'); END;
