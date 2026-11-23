CREATE TABLE operator_p2pk_claims (
 request_id TEXT PRIMARY KEY REFERENCES feed_reward_funding(request_id),
 terms_hash TEXT NOT NULL CHECK(length(terms_hash)=64)
) STRICT;
CREATE TRIGGER original_operator_p2pk BEFORE INSERT ON operator_p2pk_claims
WHEN EXISTS(SELECT 1 FROM operator_p2pk_claims WHERE request_id=NEW.request_id)
 OR EXISTS(SELECT 1 FROM operator_cashu_execution_claims WHERE request_id=NEW.request_id)
 OR EXISTS(SELECT 1 FROM payout_execution_claims WHERE invoice_id=NEW.request_id)
 OR EXISTS(SELECT 1 FROM cashu_payout_execution_claims WHERE disposition_id=NEW.request_id)
BEGIN SELECT RAISE(ABORT,'operator execution already bound'); END;
CREATE TRIGGER exclude_p2pk_operator_invoice BEFORE INSERT ON operator_cashu_execution_claims
WHEN EXISTS(SELECT 1 FROM operator_p2pk_claims WHERE request_id=NEW.request_id)
BEGIN SELECT RAISE(ABORT,'operator method is P2PK'); END;
CREATE TRIGGER exclude_p2pk_member_execution BEFORE INSERT ON cashu_payout_execution_claims
WHEN EXISTS(SELECT 1 FROM operator_p2pk_claims WHERE request_id=NEW.disposition_id)
BEGIN SELECT RAISE(ABORT,'operator disposition already bound'); END;
CREATE TRIGGER exclude_p2pk_strike_execution BEFORE INSERT ON payout_execution_claims
WHEN EXISTS(SELECT 1 FROM operator_p2pk_claims WHERE request_id=NEW.invoice_id)
BEGIN SELECT RAISE(ABORT,'operator disposition already bound'); END;
CREATE TRIGGER reserve_before_operator_p2pk BEFORE INSERT ON reward_reservations
WHEN EXISTS(SELECT 1 FROM operator_p2pk_claims WHERE request_id=NEW.request_id)
BEGIN SELECT RAISE(ABORT,'operator claim already bound'); END;
CREATE TRIGGER immutable_operator_p2pk BEFORE UPDATE ON operator_p2pk_claims
BEGIN SELECT RAISE(ABORT,'operator claim is immutable'); END;
CREATE TRIGGER retain_operator_p2pk BEFORE DELETE ON operator_p2pk_claims
BEGIN SELECT RAISE(ABORT,'operator claim is retained'); END;
