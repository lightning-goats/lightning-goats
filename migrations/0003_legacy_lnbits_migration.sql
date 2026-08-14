CREATE TABLE IF NOT EXISTS legacy_opening_credit (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    legacy_wallet_id TEXT NOT NULL CHECK (length(legacy_wallet_id) > 0),
    amount_sats INTEGER NOT NULL CHECK (amount_sats >= 0),
    cutover_at INTEGER NOT NULL CHECK (cutover_at > 0),
    snapshot_at INTEGER NOT NULL CHECK (snapshot_at >= cutover_at),
    imported_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS legacy_pending_invoices (
    payment_hash TEXT PRIMARY KEY CHECK (length(payment_hash) = 64),
    legacy_checking_id TEXT UNIQUE,
    legacy_wallet_id TEXT NOT NULL CHECK (length(legacy_wallet_id) > 0),
    amount_sats INTEGER NOT NULL CHECK (amount_sats > 0),
    legacy_created_at INTEGER,
    legacy_expiry_at INTEGER,
    snapshot_at INTEGER NOT NULL CHECK (snapshot_at > 0),
    imported_at INTEGER,
    CHECK (legacy_expiry_at IS NULL OR legacy_created_at IS NULL OR legacy_expiry_at >= legacy_created_at)
);

ALTER TABLE legacy_imports ADD COLUMN legacy_wallet_id TEXT;
ALTER TABLE legacy_imports ADD COLUMN legacy_created_at INTEGER;

CREATE UNIQUE INDEX IF NOT EXISTS legacy_imports_checking_id_unique
ON legacy_imports(legacy_checking_id)
WHERE legacy_checking_id IS NOT NULL;
