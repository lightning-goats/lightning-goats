CREATE TABLE payout_operator_methods (
 sequence INTEGER PRIMARY KEY REFERENCES payout_operator_allocations(sequence),
 method TEXT NOT NULL CHECK(method IN ('lightning','p2pk')),
 destination TEXT NOT NULL
) STRICT;
INSERT INTO payout_operator_methods SELECT sequence,'lightning',address FROM payout_operator_invoices;
CREATE TRIGGER immutable_operator_method BEFORE UPDATE ON payout_operator_methods
BEGIN SELECT RAISE(ABORT,'operator method is frozen'); END;
CREATE TRIGGER retain_operator_method BEFORE DELETE ON payout_operator_methods
BEGIN SELECT RAISE(ABORT,'operator method is retained'); END;
CREATE TRIGGER original_operator_method BEFORE INSERT ON payout_operator_methods
WHEN EXISTS(SELECT 1 FROM payout_operator_methods WHERE sequence=NEW.sequence)
BEGIN SELECT RAISE(ABORT,'operator method already bound'); END;
CREATE TRIGGER check_operator_invoice_method BEFORE INSERT ON payout_operator_invoices
WHEN EXISTS(SELECT 1 FROM payout_operator_methods WHERE sequence=NEW.sequence AND (method!='lightning' OR destination!=NEW.address))
BEGIN SELECT RAISE(ABORT,'operator method differs'); END;
CREATE TRIGGER bind_legacy_operator_method AFTER INSERT ON payout_operator_invoices
WHEN NOT EXISTS(SELECT 1 FROM payout_operator_methods WHERE sequence=NEW.sequence)
BEGIN INSERT INTO payout_operator_methods VALUES(NEW.sequence,'lightning',NEW.address); END;
CREATE TABLE payout_operator_ecash_status (
 sequence INTEGER PRIMARY KEY REFERENCES payout_operator_methods(sequence),
 state TEXT NOT NULL CHECK(state IN ('pending','held','ready')),
 reason TEXT, amount_sat INTEGER, fee_sat INTEGER,
 CHECK(state!='ready' OR (amount_sat IS NOT NULL AND amount_sat>0 AND fee_sat IS NOT NULL AND fee_sat>=0))
) STRICT;
CREATE TRIGGER final_operator_ecash_status BEFORE UPDATE ON payout_operator_ecash_status WHEN OLD.state='ready'
BEGIN SELECT RAISE(ABORT,'operator token already issued'); END;
CREATE TRIGGER retain_operator_ecash_status BEFORE DELETE ON payout_operator_ecash_status
BEGIN SELECT RAISE(ABORT,'operator status is retained'); END;
CREATE TRIGGER original_operator_ecash_status BEFORE INSERT ON payout_operator_ecash_status
WHEN EXISTS(SELECT 1 FROM payout_operator_ecash_status WHERE sequence=NEW.sequence)
 OR NOT EXISTS(SELECT 1 FROM payout_operator_methods WHERE sequence=NEW.sequence AND method='p2pk')
BEGIN SELECT RAISE(ABORT,'operator status must match original method'); END;
