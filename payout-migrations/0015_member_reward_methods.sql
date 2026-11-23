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
