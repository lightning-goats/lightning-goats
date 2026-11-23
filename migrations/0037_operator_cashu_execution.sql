CREATE TABLE operator_cashu_execution_claims (
    request_id TEXT PRIMARY KEY REFERENCES feed_reward_funding(request_id),
    invoice_hash TEXT NOT NULL UNIQUE CHECK(length(invoice_hash)=64),
    terms_hash TEXT NOT NULL CHECK(length(terms_hash)=64)
) STRICT;
CREATE TRIGGER immutable_operator_cashu_execution BEFORE UPDATE ON operator_cashu_execution_claims
BEGIN SELECT RAISE(ABORT,'operator transfer claim is immutable'); END;
CREATE TRIGGER retained_operator_cashu_execution BEFORE DELETE ON operator_cashu_execution_claims
BEGIN SELECT RAISE(ABORT,'operator transfer claim is retained'); END;
CREATE TRIGGER original_operator_cashu_execution BEFORE INSERT ON operator_cashu_execution_claims
WHEN EXISTS(SELECT 1 FROM operator_cashu_execution_claims WHERE request_id=NEW.request_id OR invoice_hash=NEW.invoice_hash)
 OR EXISTS(SELECT 1 FROM cashu_payout_execution_claims WHERE invoice_hash=NEW.invoice_hash OR disposition_id=NEW.request_id)
 OR EXISTS(SELECT 1 FROM payout_execution_claims WHERE invoice_hash=NEW.invoice_hash OR invoice_id=NEW.request_id)
BEGIN SELECT RAISE(ABORT,'operator transfer conflicts with existing execution'); END;
CREATE TRIGGER member_cashu_operator_conflict BEFORE INSERT ON cashu_payout_execution_claims
WHEN EXISTS(SELECT 1 FROM operator_cashu_execution_claims WHERE invoice_hash=NEW.invoice_hash OR request_id=NEW.disposition_id)
BEGIN SELECT RAISE(ABORT,'execution already bound to operator'); END;
CREATE TRIGGER member_strike_operator_conflict BEFORE INSERT ON payout_execution_claims
WHEN EXISTS(SELECT 1 FROM operator_cashu_execution_claims WHERE invoice_hash=NEW.invoice_hash OR request_id=NEW.invoice_id)
BEGIN SELECT RAISE(ABORT,'execution already bound to operator'); END;
CREATE TRIGGER reserve_before_operator_execution BEFORE INSERT ON reward_reservations
WHEN EXISTS(SELECT 1 FROM operator_cashu_execution_claims WHERE request_id=NEW.request_id)
BEGIN SELECT RAISE(ABORT,'operator share already claimed for this feeding'); END;
