PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS strike_receive_requests (
    receive_request_id TEXT PRIMARY KEY,
    address_user TEXT NOT NULL CHECK (length(address_user) BETWEEN 1 AND 64),
    credit_pool TEXT NOT NULL CHECK (length(credit_pool) BETWEEN 1 AND 64),
    amount_msat INTEGER NOT NULL CHECK (amount_msat > 0),
    description_hash TEXT NOT NULL CHECK (length(description_hash) = 64),
    payment_hash TEXT NOT NULL UNIQUE CHECK (length(payment_hash) = 64),
    invoice TEXT NOT NULL CHECK (length(invoice) BETWEEN 1 AND 8192),
    created_provider TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS strike_receive_requests_address_user_idx
ON strike_receive_requests(address_user, created_at);
