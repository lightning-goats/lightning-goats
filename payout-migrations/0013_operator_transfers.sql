-- Only invoices committed here may be submitted. A missing receipt leaves the
-- same feed eligible for reconciliation after a process or network failure.
CREATE TABLE payout_operator_invoices (
    sequence INTEGER PRIMARY KEY REFERENCES payout_operator_allocations(sequence),
    address TEXT NOT NULL,
    invoice TEXT NOT NULL UNIQUE,
    payment_hash TEXT NOT NULL UNIQUE CHECK(length(payment_hash)=64),
    amount_sat INTEGER NOT NULL CHECK(amount_sat>0),
    maximum_fee_sat INTEGER NOT NULL CHECK(maximum_fee_sat>=0)
) STRICT;
CREATE TRIGGER bounded_operator_invoice BEFORE INSERT ON payout_operator_invoices
WHEN EXISTS(SELECT 1 FROM payout_operator_invoices WHERE sequence=NEW.sequence OR invoice=NEW.invoice OR payment_hash=NEW.payment_hash)
 OR NEW.amount_sat + NEW.maximum_fee_sat > (SELECT operator_sats FROM payout_operator_allocations WHERE sequence=NEW.sequence)
BEGIN SELECT RAISE(ABORT,'operator invoice conflicts with allocation'); END;
CREATE TRIGGER immutable_operator_invoice BEFORE UPDATE ON payout_operator_invoices
BEGIN SELECT RAISE(ABORT,'operator invoice is immutable'); END;
CREATE TRIGGER retained_operator_invoice BEFORE DELETE ON payout_operator_invoices
BEGIN SELECT RAISE(ABORT,'operator invoice is retained'); END;

CREATE TABLE payout_operator_receipts (
    sequence INTEGER PRIMARY KEY REFERENCES payout_operator_invoices(sequence),
    payment_preimage TEXT NOT NULL CHECK(length(payment_preimage)=64),
    fee_sat INTEGER NOT NULL CHECK(fee_sat>=0)
) STRICT;
CREATE TRIGGER bounded_operator_receipt BEFORE INSERT ON payout_operator_receipts
WHEN EXISTS(SELECT 1 FROM payout_operator_receipts WHERE sequence=NEW.sequence)
 OR NEW.fee_sat > (SELECT maximum_fee_sat FROM payout_operator_invoices WHERE sequence=NEW.sequence)
BEGIN SELECT RAISE(ABORT,'operator receipt conflicts with invoice'); END;
CREATE TRIGGER immutable_operator_receipt BEFORE UPDATE ON payout_operator_receipts
BEGIN SELECT RAISE(ABORT,'operator receipt is immutable'); END;
CREATE TRIGGER retained_operator_receipt BEFORE DELETE ON payout_operator_receipts
BEGIN SELECT RAISE(ABORT,'operator receipt is retained'); END;
