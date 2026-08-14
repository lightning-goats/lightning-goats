PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS cln_cursor (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    last_pay_index INTEGER NOT NULL CHECK (last_pay_index >= 0),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS settled_invoices (
    pay_index INTEGER PRIMARY KEY CHECK (pay_index >= 0),
    payment_hash TEXT NOT NULL UNIQUE,
    label TEXT,
    amount_msat INTEGER NOT NULL CHECK (amount_msat >= 0),
    classified_user TEXT,
    credited_sats INTEGER NOT NULL DEFAULT 0 CHECK (credited_sats >= 0),
    settled_at INTEGER,
    received_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS ledger_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entry_type TEXT NOT NULL,
    source_key TEXT NOT NULL UNIQUE,
    delta_sats INTEGER NOT NULL,
    payment_hash TEXT,
    feed_attempt_id TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    FOREIGN KEY (payment_hash) REFERENCES settled_invoices(payment_hash)
);

CREATE TABLE IF NOT EXISTS feed_attempts (
    id TEXT PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('intent_committed', 'confirmed', 'unknown', 'reconciled_not_fed')),
    threshold_sats INTEGER NOT NULL CHECK (threshold_sats > 0),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    resolved_at INTEGER,
    error TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS one_unresolved_feed_attempt
ON feed_attempts((1))
WHERE status IN ('intent_committed', 'unknown');

CREATE TABLE IF NOT EXISTS event_log (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS message_outbox (
    event_id TEXT PRIMARY KEY,
    signed_event_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'published', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    last_error TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    published_at INTEGER
);

CREATE TABLE IF NOT EXISTS legacy_imports (
    payment_hash TEXT PRIMARY KEY,
    legacy_checking_id TEXT,
    amount_sats INTEGER NOT NULL CHECK (amount_sats >= 0),
    settled_at INTEGER,
    imported_at INTEGER NOT NULL DEFAULT (unixepoch())
);
