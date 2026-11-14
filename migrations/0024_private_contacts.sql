-- Private fees never enter settled_payments, feed credit, reward funding or event_log.
CREATE TABLE private_contact_intents (
    id TEXT PRIMARY KEY CHECK(length(id)=36),
    capability_hash TEXT NOT NULL UNIQUE CHECK(length(capability_hash)=64),
    reply_email_hash TEXT NOT NULL CHECK(length(reply_email_hash)=64),
    envelope_id TEXT NOT NULL UNIQUE CHECK(length(envelope_id)=64),
    envelope_json TEXT NOT NULL CHECK(length(envelope_json) BETWEEN 1 AND 24576),
    recipient TEXT NOT NULL CHECK(length(recipient)=64),
    description_hash TEXT NOT NULL UNIQUE CHECK(length(description_hash)=64),
    relays_json TEXT NOT NULL CHECK(length(relays_json) BETWEEN 1 AND 16384),
    state TEXT NOT NULL DEFAULT 'new' CHECK(state IN ('new','issuing','issued','uncertain')),
    issuance_claim TEXT,
    receive_request_id TEXT UNIQUE REFERENCES strike_receive_requests(receive_request_id),
    created_at INTEGER NOT NULL DEFAULT(unixepoch()),
    CHECK((state='issued')=(receive_request_id IS NOT NULL)),
    CHECK((state IN ('issuing','uncertain','issued'))=(issuance_claim IS NOT NULL))
);
CREATE INDEX private_contact_admission ON private_contact_intents(created_at);

CREATE TABLE private_contact_fees (
    contact_id TEXT PRIMARY KEY REFERENCES private_contact_intents(id),
    receive_id TEXT NOT NULL UNIQUE CHECK(length(receive_id)=36),
    invoice_payment_hash TEXT NOT NULL UNIQUE CHECK(length(invoice_payment_hash)=64),
    amount_msat INTEGER NOT NULL CHECK(amount_msat=100000),
    accepted_at INTEGER NOT NULL DEFAULT(unixepoch())
);

CREATE TABLE private_contact_delivery (
    contact_id TEXT PRIMARY KEY REFERENCES private_contact_fees(contact_id),
    lease_id TEXT,
    lease_until INTEGER,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts>=0),
    next_attempt INTEGER NOT NULL DEFAULT(unixepoch()),
    accepted_relay TEXT,
    accepted_at INTEGER,
    CHECK((lease_id IS NULL)=(lease_until IS NULL)),
    CHECK((accepted_relay IS NULL)=(accepted_at IS NULL))
);
CREATE INDEX private_contact_delivery_due ON private_contact_delivery(accepted_at,next_attempt);

CREATE TRIGGER private_contact_intent_immutable BEFORE UPDATE ON private_contact_intents
WHEN NEW.id IS NOT OLD.id OR NEW.capability_hash IS NOT OLD.capability_hash
 OR NEW.reply_email_hash IS NOT OLD.reply_email_hash OR NEW.envelope_id IS NOT OLD.envelope_id
 OR NEW.envelope_json IS NOT OLD.envelope_json OR NEW.recipient IS NOT OLD.recipient
 OR NEW.description_hash IS NOT OLD.description_hash OR NEW.relays_json IS NOT OLD.relays_json
 OR NEW.created_at IS NOT OLD.created_at
 OR (OLD.issuance_claim IS NOT NULL AND NEW.issuance_claim IS NOT OLD.issuance_claim)
 OR (OLD.receive_request_id IS NOT NULL AND NEW.receive_request_id IS NOT OLD.receive_request_id)
 OR (NEW.state IS NOT OLD.state AND NOT (
    (OLD.state='new' AND NEW.state='issuing') OR
    (OLD.state='issuing' AND NEW.state IN ('uncertain','issued')) OR
    (OLD.state='uncertain' AND NEW.state='issued')))
BEGIN SELECT RAISE(ABORT,'private contact intent is immutable and issuance is monotone'); END;
CREATE TRIGGER private_contact_intent_retained BEFORE DELETE ON private_contact_intents
BEGIN SELECT RAISE(ABORT,'private contact identity is retained'); END;
CREATE TRIGGER private_contact_fee_immutable BEFORE UPDATE ON private_contact_fees
BEGIN SELECT RAISE(ABORT,'private fee is immutable'); END;
CREATE TRIGGER private_contact_fee_retained BEFORE DELETE ON private_contact_fees
BEGIN SELECT RAISE(ABORT,'private fee is retained'); END;
CREATE TRIGGER private_contact_delivery_monotone BEFORE UPDATE ON private_contact_delivery
WHEN NEW.contact_id IS NOT OLD.contact_id OR NEW.attempts<OLD.attempts
 OR (OLD.accepted_at IS NOT NULL AND (NEW.accepted_at IS NOT OLD.accepted_at OR NEW.accepted_relay IS NOT OLD.accepted_relay))
BEGIN SELECT RAISE(ABORT,'private delivery identity and acceptance are retained'); END;
CREATE TRIGGER private_contact_delivery_retained BEFORE DELETE ON private_contact_delivery
BEGIN SELECT RAISE(ABORT,'private delivery replay identity is retained'); END;
