-- Historical rows retain their invoice route across restart and rollback.
ALTER TABLE payout_rows ADD COLUMN reward_method TEXT NOT NULL DEFAULT 'lightning'
    CHECK(reward_method IN ('lightning','public_nutzap','private_ecash'));

CREATE TRIGGER bind_member_ecash_destination BEFORE INSERT ON payout_rows
WHEN NEW.reward_method != 'lightning' AND (
    json_extract(NEW.recipient_json,'$.method') IS NOT NEW.reward_method OR
    json_extract(NEW.recipient_json,'$.receiving_key') IS NOT NEW.address
)
BEGIN SELECT RAISE(ABORT,'ecash destination differs from assignment'); END;

CREATE TRIGGER forbid_member_ecash_invoice BEFORE INSERT ON payout_invoice_intents
WHEN (SELECT reward_method FROM payout_rows WHERE grant_id=NEW.grant_id AND member=NEW.member) != 'lightning'
BEGIN SELECT RAISE(ABORT,'member allocation is bound to ecash'); END;
CREATE TRIGGER forbid_member_ecash_fee_probe BEFORE INSERT ON payout_fee_probe_intents
WHEN (SELECT reward_method FROM payout_rows WHERE grant_id=NEW.grant_id AND member=NEW.member) != 'lightning'
BEGIN SELECT RAISE(ABORT,'member allocation is bound to ecash'); END;
CREATE TRIGGER forbid_member_ecash_invoice_closure BEFORE INSERT ON payout_unsubmitted
WHEN (SELECT reward_method FROM payout_rows WHERE grant_id=NEW.grant_id AND member=NEW.member) != 'lightning'
BEGIN SELECT RAISE(ABORT,'ecash disposition needs its own settlement evidence'); END;
