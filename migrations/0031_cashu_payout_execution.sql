CREATE TABLE cashu_payout_execution_claims (
    reservation_id TEXT NOT NULL REFERENCES reward_reservations(id),
    member TEXT NOT NULL,
    disposition_id TEXT NOT NULL UNIQUE,
    invoice_hash TEXT NOT NULL UNIQUE,
    gross_sats INTEGER NOT NULL CHECK(typeof(gross_sats)='integer' AND gross_sats>0),
    document_json TEXT NOT NULL,
    created_at INTEGER NOT NULL CHECK(created_at>=0),
    PRIMARY KEY(reservation_id,member)
);
CREATE TRIGGER immutable_cashu_payout_execution BEFORE UPDATE ON cashu_payout_execution_claims
BEGIN SELECT RAISE(ABORT,'Cashu execution claim is immutable'); END;
CREATE TRIGGER retain_cashu_payout_execution BEFORE DELETE ON cashu_payout_execution_claims
BEGIN SELECT RAISE(ABORT,'Cashu execution claim is retained'); END;
CREATE TRIGGER cashu_execution_provider_conflict BEFORE INSERT ON cashu_payout_execution_claims
WHEN EXISTS(SELECT 1 FROM payout_execution_claims WHERE reservation_id=NEW.reservation_id
            OR invoice_id=NEW.disposition_id OR invoice_hash=NEW.invoice_hash)
BEGIN SELECT RAISE(ABORT,'execution already bound to Strike'); END;
CREATE TRIGGER strike_execution_provider_conflict BEFORE INSERT ON payout_execution_claims
WHEN EXISTS(SELECT 1 FROM cashu_payout_execution_claims WHERE reservation_id=NEW.reservation_id
            OR disposition_id=NEW.invoice_id OR invoice_hash=NEW.invoice_hash)
BEGIN SELECT RAISE(ABORT,'execution already bound to Cashu'); END;
