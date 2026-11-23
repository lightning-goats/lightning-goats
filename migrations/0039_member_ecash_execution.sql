CREATE TABLE member_ecash_execution_claims (
    reservation_id TEXT NOT NULL REFERENCES reward_reservations(id),
    member TEXT NOT NULL,
    gross_sats INTEGER NOT NULL CHECK(gross_sats>0),
    terms_hash TEXT NOT NULL UNIQUE CHECK(length(terms_hash)=64),
    PRIMARY KEY(reservation_id,member)
) STRICT;
CREATE TRIGGER member_ecash_claim_budget BEFORE INSERT ON member_ecash_execution_claims
WHEN EXISTS(SELECT 1 FROM payout_execution_claims WHERE reservation_id=NEW.reservation_id)
 OR EXISTS(SELECT 1 FROM cashu_payout_execution_claims WHERE reservation_id=NEW.reservation_id AND member=NEW.member)
 OR EXISTS(SELECT 1 FROM reward_outcome_manifests WHERE reservation_id=NEW.reservation_id)
 OR NEW.gross_sats > (SELECT reserved_sats FROM reward_reservations WHERE id=NEW.reservation_id)
    - (SELECT COALESCE(SUM(gross_sats),0) FROM cashu_payout_execution_claims WHERE reservation_id=NEW.reservation_id)
    - (SELECT COALESCE(SUM(gross_sats),0) FROM member_ecash_execution_claims WHERE reservation_id=NEW.reservation_id)
BEGIN SELECT RAISE(ABORT,'member ecash exceeds available reservation or conflicts with an execution'); END;
CREATE TRIGGER member_invoice_after_ecash BEFORE INSERT ON cashu_payout_execution_claims
WHEN EXISTS(SELECT 1 FROM member_ecash_execution_claims WHERE reservation_id=NEW.reservation_id AND member=NEW.member)
 OR NEW.gross_sats > (SELECT reserved_sats FROM reward_reservations WHERE id=NEW.reservation_id)
    - (SELECT COALESCE(SUM(gross_sats),0) FROM cashu_payout_execution_claims WHERE reservation_id=NEW.reservation_id)
    - (SELECT COALESCE(SUM(gross_sats),0) FROM member_ecash_execution_claims WHERE reservation_id=NEW.reservation_id)
BEGIN SELECT RAISE(ABORT,'invoice execution conflicts with reserved member funds'); END;
CREATE TRIGGER member_provider_after_ecash BEFORE INSERT ON payout_execution_claims
WHEN EXISTS(SELECT 1 FROM member_ecash_execution_claims WHERE reservation_id=NEW.reservation_id)
BEGIN SELECT RAISE(ABORT,'member grant is bound to retained Cashu'); END;
CREATE TRIGGER immutable_member_ecash_claim BEFORE UPDATE ON member_ecash_execution_claims
BEGIN SELECT RAISE(ABORT,'member ecash authorization is immutable'); END;
CREATE TRIGGER retain_member_ecash_claim BEFORE DELETE ON member_ecash_execution_claims
BEGIN SELECT RAISE(ABORT,'member ecash authorization is retained'); END;
