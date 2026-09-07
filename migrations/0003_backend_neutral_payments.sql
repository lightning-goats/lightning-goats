PRAGMA foreign_keys = ON;

-- Canonical provider-neutral payment settlement table. Legacy CLN tables remain
-- temporarily for the compatibility watcher and are removed in Phase 1 issue #13.
CREATE TABLE IF NOT EXISTS settled_payments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL CHECK (length(source) BETWEEN 1 AND 32),
    source_id TEXT NOT NULL CHECK (length(source_id) BETWEEN 1 AND 256),
    payment_hash TEXT UNIQUE,
    address_user TEXT NOT NULL CHECK (length(address_user) BETWEEN 1 AND 64),
    credit_pool TEXT NOT NULL CHECK (length(credit_pool) BETWEEN 1 AND 64),
    amount_msat INTEGER NOT NULL CHECK (amount_msat > 0),
    settled_at INTEGER CHECK (settled_at IS NULL OR settled_at >= 0),
    context_json TEXT,
    received_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (source, source_id)
);

CREATE INDEX IF NOT EXISTS settled_payments_address_user_idx
ON settled_payments(address_user, received_at);

-- The original ledger_entries.payment_hash foreign key pointed exclusively at
-- the legacy CLN settled_invoices table. Rebuild the table so provider-neutral
-- settlements can be referenced without coupling the financial ledger to CLN.
CREATE TABLE ledger_entries_v2 (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entry_type TEXT NOT NULL,
    source_key TEXT NOT NULL UNIQUE,
    delta_sats INTEGER NOT NULL,
    payment_hash TEXT,
    payment_source TEXT,
    payment_source_id TEXT,
    address_user TEXT,
    credit_pool TEXT,
    feed_attempt_id TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

INSERT INTO ledger_entries_v2
    (id, entry_type, source_key, delta_sats, payment_hash, feed_attempt_id, created_at)
SELECT
    id, entry_type, source_key, delta_sats, payment_hash, feed_attempt_id, created_at
FROM ledger_entries;

DROP TABLE ledger_entries;
ALTER TABLE ledger_entries_v2 RENAME TO ledger_entries;

CREATE INDEX IF NOT EXISTS ledger_entries_payment_source_idx
ON ledger_entries(payment_source, payment_source_id);
