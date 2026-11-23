-- Independent core history fences payout-only snapshot rollback. Claims never
-- expire or disappear merely because a quote/invoice or worker has expired.
CREATE TABLE payout_execution_claims (
    reservation_id TEXT NOT NULL REFERENCES reward_reservations(id),
    member TEXT NOT NULL,
    id TEXT NOT NULL UNIQUE,
    document_json TEXT NOT NULL,
    scope_json TEXT NOT NULL,
    quote_id TEXT NOT NULL UNIQUE,
    quote_intent_id TEXT NOT NULL UNIQUE,
    invoice_id TEXT NOT NULL UNIQUE,
    invoice_hash TEXT NOT NULL UNIQUE,
    gross_sats INTEGER NOT NULL CHECK(typeof(gross_sats)='integer' AND gross_sats>0),
    created_at INTEGER NOT NULL CHECK(created_at>=0),
    PRIMARY KEY(reservation_id,member)
);
CREATE TRIGGER immutable_payout_execution_claim BEFORE UPDATE ON payout_execution_claims
BEGIN SELECT RAISE(ABORT,'original execution claim is immutable'); END;
CREATE TRIGGER retained_payout_execution_claim BEFORE DELETE ON payout_execution_claims
BEGIN SELECT RAISE(ABORT,'original execution claim is retained'); END;
