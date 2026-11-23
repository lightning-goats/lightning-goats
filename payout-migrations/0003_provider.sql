-- Quote retirement is possible only before any execute intent for that invoice.
CREATE TABLE payout_quote_intents (
    id TEXT PRIMARY KEY,
    invoice_id TEXT NOT NULL REFERENCES payout_invoices(intent_id),
    document_json TEXT NOT NULL,
    retired_at INTEGER CHECK(retired_at IS NULL OR retired_at>=0)
);
CREATE UNIQUE INDEX one_current_payout_quote ON payout_quote_intents(invoice_id) WHERE retired_at IS NULL;
CREATE TABLE payout_quote_records (
    intent_id TEXT PRIMARY KEY REFERENCES payout_quote_intents(id),
    provider_quote_id TEXT NOT NULL UNIQUE,
    document_json TEXT NOT NULL
);
CREATE TABLE payout_quote_duplicates (
    intent_id TEXT PRIMARY KEY REFERENCES payout_quote_intents(id),
    provider_quote_id TEXT NOT NULL
);
CREATE TABLE payout_execute_intents (
    intent_id TEXT PRIMARY KEY REFERENCES payout_quote_records(intent_id),
    document_json TEXT NOT NULL,
    created_at INTEGER NOT NULL CHECK(created_at>=0)
);
CREATE TABLE payout_payment_bindings (
    payment_id TEXT PRIMARY KEY,
    intent_id TEXT NOT NULL UNIQUE REFERENCES payout_execute_intents(intent_id),
    document_json TEXT NOT NULL,
    bound_at INTEGER NOT NULL CHECK(bound_at>=0)
);
CREATE TABLE payout_payment_records (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    payment_id TEXT NOT NULL REFERENCES payout_payment_bindings(payment_id),
    state TEXT NOT NULL CHECK(state IN ('PENDING','COMPLETED','FAILED')),
    document_json TEXT NOT NULL,
    observed_at INTEGER NOT NULL CHECK(observed_at>=0)
);
CREATE TABLE payout_payment_conflicts (
    payment_id TEXT PRIMARY KEY REFERENCES payout_payment_bindings(payment_id),
    document_json TEXT NOT NULL,
    observed_at INTEGER NOT NULL CHECK(observed_at>=0)
);
CREATE TABLE payout_provider_journal (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    intent_id TEXT NOT NULL REFERENCES payout_quote_intents(id),
    stage TEXT NOT NULL CHECK(stage IN ('quote_intent','quote_frozen','duplicate_quote','retired','execute_intent','payment_bound','payment_observed','conflict')),
    created_at INTEGER NOT NULL CHECK(created_at>=0)
);
CREATE TRIGGER freeze_quote_request BEFORE UPDATE ON payout_quote_intents
WHEN NEW.id IS NOT OLD.id OR NEW.invoice_id IS NOT OLD.invoice_id OR NEW.document_json IS NOT OLD.document_json
 OR OLD.retired_at IS NOT NULL OR NEW.retired_at IS NULL
 OR EXISTS(SELECT 1 FROM payout_execute_intents e JOIN payout_quote_intents q ON q.id=e.intent_id WHERE q.invoice_id=OLD.invoice_id)
BEGIN SELECT RAISE(ABORT,'only a never-executed quote may retire once'); END;
CREATE TRIGGER retained_quote_request BEFORE DELETE ON payout_quote_intents
BEGIN SELECT RAISE(ABORT,'quote identities are retained'); END;
CREATE TRIGGER immutable_quote_records BEFORE UPDATE ON payout_quote_records
BEGIN SELECT RAISE(ABORT,'quote terms are frozen'); END;
CREATE TRIGGER retained_quote_records BEFORE DELETE ON payout_quote_records
BEGIN SELECT RAISE(ABORT,'quote terms are retained'); END;
CREATE TRIGGER immutable_quote_duplicates BEFORE UPDATE ON payout_quote_duplicates
BEGIN SELECT RAISE(ABORT,'duplicate quote observation is frozen'); END;
CREATE TRIGGER retained_quote_duplicates BEFORE DELETE ON payout_quote_duplicates
BEGIN SELECT RAISE(ABORT,'duplicate quote observation is retained'); END;
CREATE TRIGGER immutable_execute_intents BEFORE UPDATE ON payout_execute_intents
BEGIN SELECT RAISE(ABORT,'execution intent is frozen'); END;
CREATE TRIGGER one_executable_quote_per_invoice BEFORE INSERT ON payout_execute_intents
WHEN EXISTS(SELECT 1 FROM payout_quote_intents WHERE id=NEW.intent_id AND retired_at IS NOT NULL)
 OR EXISTS(SELECT 1 FROM payout_execute_intents e JOIN payout_quote_intents old ON old.id=e.intent_id
 JOIN payout_quote_intents current ON current.invoice_id=old.invoice_id WHERE current.id=NEW.intent_id)
BEGIN SELECT RAISE(ABORT,'an invoice cannot acquire a second execution identity'); END;
CREATE TRIGGER retained_execute_intents BEFORE DELETE ON payout_execute_intents
BEGIN SELECT RAISE(ABORT,'uncertain execution cannot be erased'); END;
CREATE TRIGGER immutable_payment_bindings BEFORE UPDATE ON payout_payment_bindings
BEGIN SELECT RAISE(ABORT,'original payment identity is frozen'); END;
CREATE TRIGGER retained_payment_bindings BEFORE DELETE ON payout_payment_bindings
BEGIN SELECT RAISE(ABORT,'original payment identity is retained'); END;
CREATE TRIGGER immutable_payment_records BEFORE UPDATE ON payout_payment_records
BEGIN SELECT RAISE(ABORT,'payment observations are append-only'); END;
CREATE TRIGGER retained_payment_records BEFORE DELETE ON payout_payment_records
BEGIN SELECT RAISE(ABORT,'payment observations are retained'); END;
CREATE TRIGGER immutable_payment_conflicts BEFORE UPDATE ON payout_payment_conflicts
BEGIN SELECT RAISE(ABORT,'terminal conflict requires explicit recovery'); END;
CREATE TRIGGER retained_payment_conflicts BEFORE DELETE ON payout_payment_conflicts
BEGIN SELECT RAISE(ABORT,'terminal conflict is retained'); END;
CREATE TRIGGER immutable_provider_journal BEFORE UPDATE ON payout_provider_journal
BEGIN SELECT RAISE(ABORT,'provider journal is append-only'); END;
CREATE TRIGGER retained_provider_journal BEFORE DELETE ON payout_provider_journal
BEGIN SELECT RAISE(ABORT,'provider journal is retained'); END;
