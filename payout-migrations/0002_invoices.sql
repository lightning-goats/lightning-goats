-- Persist before signing or issuing an outgoing LNURL callback. No execution.
CREATE TABLE payout_invoice_intents (
    id TEXT PRIMARY KEY,
    grant_id TEXT NOT NULL,
    member TEXT NOT NULL,
    document_json TEXT NOT NULL,
    UNIQUE(grant_id,member),
    FOREIGN KEY(grant_id,member) REFERENCES payout_rows(grant_id,member)
);
CREATE TABLE payout_callback_requests (
    intent_id TEXT PRIMARY KEY REFERENCES payout_invoice_intents(id),
    protocol TEXT NOT NULL CHECK(protocol IN ('ordinary','nip57')),
    raw_zap TEXT,
    created_at INTEGER NOT NULL CHECK(created_at>=0),
    CHECK((protocol='nip57')=(raw_zap IS NOT NULL))
);
CREATE TABLE payout_invoices (
    intent_id TEXT PRIMARY KEY REFERENCES payout_callback_requests(intent_id),
    payment_hash TEXT NOT NULL UNIQUE,
    encoded TEXT NOT NULL,
    document_json TEXT NOT NULL,
    validated_at INTEGER NOT NULL CHECK(validated_at>=0)
);
CREATE TABLE payout_invoice_journal (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    intent_id TEXT NOT NULL REFERENCES payout_invoice_intents(id),
    stage TEXT NOT NULL CHECK(stage IN ('intent','callback','invoice')),
    created_at INTEGER NOT NULL CHECK(created_at>=0),
    UNIQUE(intent_id,stage)
);
CREATE TRIGGER immutable_invoice_intents BEFORE UPDATE ON payout_invoice_intents
BEGIN SELECT RAISE(ABORT,'invoice intents are frozen'); END;
CREATE TRIGGER retained_invoice_intents BEFORE DELETE ON payout_invoice_intents
BEGIN SELECT RAISE(ABORT,'invoice intents are retained'); END;
CREATE TRIGGER immutable_callback_requests BEFORE UPDATE ON payout_callback_requests
BEGIN SELECT RAISE(ABORT,'callback bytes are frozen'); END;
CREATE TRIGGER retained_callback_requests BEFORE DELETE ON payout_callback_requests
BEGIN SELECT RAISE(ABORT,'callback bytes are retained'); END;
CREATE TRIGGER immutable_payout_invoices BEFORE UPDATE ON payout_invoices
BEGIN SELECT RAISE(ABORT,'outgoing invoice identity is frozen'); END;
CREATE TRIGGER retained_payout_invoices BEFORE DELETE ON payout_invoices
BEGIN SELECT RAISE(ABORT,'outgoing invoices are retained'); END;
CREATE TRIGGER immutable_invoice_journal BEFORE UPDATE ON payout_invoice_journal
BEGIN SELECT RAISE(ABORT,'invoice journal is append-only'); END;
CREATE TRIGGER retained_invoice_journal BEFORE DELETE ON payout_invoice_journal
BEGIN SELECT RAISE(ABORT,'invoice journal is retained'); END;
