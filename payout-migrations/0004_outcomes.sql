CREATE TABLE payout_unsubmitted (
    grant_id TEXT NOT NULL,
    member TEXT NOT NULL,
    reason TEXT NOT NULL,
    closed_at INTEGER NOT NULL CHECK(closed_at>=0),
    PRIMARY KEY(grant_id,member),
    FOREIGN KEY(grant_id,member) REFERENCES payout_rows(grant_id,member)
);
CREATE TABLE payout_outcome_manifests (
    grant_id TEXT PRIMARY KEY REFERENCES payout_grants(id),
    header_json TEXT NOT NULL,
    rows_json TEXT NOT NULL,
    created_at INTEGER NOT NULL CHECK(created_at>=0)
);
CREATE TABLE payout_outcome_acks (
    grant_id TEXT PRIMARY KEY REFERENCES payout_outcome_manifests(grant_id),
    progress_json TEXT NOT NULL,
    spent_sats INTEGER NOT NULL CHECK(typeof(spent_sats)='integer' AND spent_sats>=0),
    held_sats INTEGER NOT NULL CHECK(typeof(held_sats)='integer' AND held_sats>=0),
    accepted_at INTEGER NOT NULL CHECK(accepted_at>=0)
);
CREATE TRIGGER no_close_after_possible_execute BEFORE INSERT ON payout_unsubmitted
WHEN EXISTS(SELECT 1 FROM payout_invoice_intents i JOIN payout_quote_intents q ON q.invoice_id=i.id
 JOIN payout_execute_intents e ON e.intent_id=q.id WHERE i.grant_id=NEW.grant_id AND i.member=NEW.member)
 OR EXISTS(SELECT 1 FROM payout_outcome_manifests WHERE grant_id=NEW.grant_id)
BEGIN SELECT RAISE(ABORT,'possible execution or frozen outcome forbids unsubmitted closure'); END;
CREATE TRIGGER no_invoice_for_closed_row BEFORE INSERT ON payout_invoice_intents
WHEN EXISTS(SELECT 1 FROM payout_unsubmitted WHERE grant_id=NEW.grant_id AND member=NEW.member)
 OR EXISTS(SELECT 1 FROM payout_outcome_manifests WHERE grant_id=NEW.grant_id)
BEGIN SELECT RAISE(ABORT,'closed row cannot create an invoice'); END;
CREATE TRIGGER no_quote_for_closed_row BEFORE INSERT ON payout_quote_intents
WHEN EXISTS(SELECT 1 FROM payout_invoice_intents i JOIN payout_unsubmitted c ON c.grant_id=i.grant_id AND c.member=i.member WHERE i.id=NEW.invoice_id)
 OR EXISTS(SELECT 1 FROM payout_invoice_intents i JOIN payout_outcome_manifests m ON m.grant_id=i.grant_id WHERE i.id=NEW.invoice_id)
BEGIN SELECT RAISE(ABORT,'closed row cannot create a quote'); END;
CREATE TRIGGER no_execute_for_closed_row BEFORE INSERT ON payout_execute_intents
WHEN EXISTS(SELECT 1 FROM payout_quote_intents q JOIN payout_invoice_intents i ON i.id=q.invoice_id
 JOIN payout_unsubmitted c ON c.grant_id=i.grant_id AND c.member=i.member WHERE q.id=NEW.intent_id)
 OR EXISTS(SELECT 1 FROM payout_quote_intents q JOIN payout_invoice_intents i ON i.id=q.invoice_id
 JOIN payout_outcome_manifests m ON m.grant_id=i.grant_id WHERE q.id=NEW.intent_id)
BEGIN SELECT RAISE(ABORT,'closed row cannot create execution intent'); END;
CREATE TRIGGER payout_closure_immutable BEFORE UPDATE ON payout_unsubmitted
BEGIN SELECT RAISE(ABORT,'closure is immutable'); END;
CREATE TRIGGER payout_closure_retained BEFORE DELETE ON payout_unsubmitted
BEGIN SELECT RAISE(ABORT,'closure is retained'); END;
CREATE TRIGGER payout_manifest_immutable BEFORE UPDATE ON payout_outcome_manifests
BEGIN SELECT RAISE(ABORT,'outcome manifest is immutable'); END;
CREATE TRIGGER payout_manifest_retained BEFORE DELETE ON payout_outcome_manifests
BEGIN SELECT RAISE(ABORT,'outcome manifest is retained'); END;
CREATE TRIGGER payout_ack_immutable BEFORE UPDATE ON payout_outcome_acks
BEGIN SELECT RAISE(ABORT,'core acknowledgement is immutable'); END;
CREATE TRIGGER payout_ack_retained BEFORE DELETE ON payout_outcome_acks
BEGIN SELECT RAISE(ABORT,'core acknowledgement is retained'); END;
