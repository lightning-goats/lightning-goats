-- New methods are tagged in immutable recipient_json. Historical LNURL recipients have no method field.
CREATE TRIGGER bind_member_ecash_destination BEFORE INSERT ON payout_rows
WHEN json_type(NEW.recipient_json,'$.method') IS NOT NULL AND (
    COALESCE(json_extract(NEW.recipient_json,'$.method'),'') NOT IN ('public_nutzap','private_ecash') OR
    json_extract(NEW.recipient_json,'$.receiving_key') IS NOT NEW.address
)
BEGIN SELECT RAISE(ABORT,'ecash destination differs from assignment'); END;

CREATE TRIGGER forbid_member_ecash_invoice BEFORE INSERT ON payout_invoice_intents
WHEN (SELECT json_type(recipient_json,'$.method') FROM payout_rows WHERE grant_id=NEW.grant_id AND member=NEW.member) IS NOT NULL
BEGIN SELECT RAISE(ABORT,'member allocation is bound to ecash'); END;
CREATE TRIGGER forbid_member_ecash_fee_probe BEFORE INSERT ON payout_fee_probe_intents
WHEN (SELECT json_type(recipient_json,'$.method') FROM payout_rows WHERE grant_id=NEW.grant_id AND member=NEW.member) IS NOT NULL
BEGIN SELECT RAISE(ABORT,'member allocation is bound to ecash'); END;
CREATE TRIGGER forbid_member_ecash_invoice_closure BEFORE INSERT ON payout_unsubmitted
WHEN (SELECT json_type(recipient_json,'$.method') FROM payout_rows WHERE grant_id=NEW.grant_id AND member=NEW.member) IS NOT NULL
BEGIN SELECT RAISE(ABORT,'ecash disposition needs its own settlement evidence'); END;

CREATE TABLE member_ecash_ready (
    grant_id TEXT NOT NULL, member TEXT NOT NULL, terms_hash TEXT NOT NULL CHECK(length(terms_hash)=64),
    amount_sat INTEGER NOT NULL CHECK(amount_sat>0), fee_sat INTEGER NOT NULL CHECK(fee_sat>=0),
    PRIMARY KEY(grant_id,member),
    FOREIGN KEY(grant_id,member) REFERENCES payout_rows(grant_id,member)
) STRICT;
CREATE TRIGGER bind_member_ecash_ready BEFORE INSERT ON member_ecash_ready
WHEN NOT EXISTS(SELECT 1 FROM payout_rows r WHERE r.grant_id=NEW.grant_id AND r.member=NEW.member
    AND json_extract(r.recipient_json,'$.method') IN ('private_ecash','public_nutzap')
    AND NEW.amount_sat<=r.gross_sats-NEW.fee_sat)
BEGIN SELECT RAISE(ABORT,'receipt exceeds frozen member ecash allocation'); END;
CREATE TRIGGER immutable_member_ecash_ready BEFORE UPDATE ON member_ecash_ready
BEGIN SELECT RAISE(ABORT,'member ecash receipt is immutable'); END;
CREATE TRIGGER retain_member_ecash_ready BEFORE DELETE ON member_ecash_ready
BEGIN SELECT RAISE(ABORT,'member ecash receipt is retained'); END;
