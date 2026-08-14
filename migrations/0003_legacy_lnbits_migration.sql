CREATE TABLE IF NOT EXISTS legacy_opening_credit (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    legacy_wallet_id TEXT NOT NULL CHECK (length(legacy_wallet_id) > 0),
    amount_sats INTEGER NOT NULL CHECK (amount_sats >= 0),
    cutover_at INTEGER NOT NULL CHECK (cutover_at > 0),
    imported_at INTEGER NOT NULL DEFAULT (unixepoch())
);

ALTER TABLE legacy_imports ADD COLUMN legacy_wallet_id TEXT;
ALTER TABLE legacy_imports ADD COLUMN legacy_created_at INTEGER;

CREATE UNIQUE INDEX IF NOT EXISTS legacy_imports_checking_id_unique
ON legacy_imports(legacy_checking_id)
WHERE legacy_checking_id IS NOT NULL;
