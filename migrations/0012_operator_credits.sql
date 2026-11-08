-- Local operator grants are auditable feed credit, never provider settlements.
CREATE TABLE operator_credit_grants (
    grant_id TEXT PRIMARY KEY CHECK (length(grant_id) BETWEEN 1 AND 64),
    amount_sats INTEGER NOT NULL CHECK (amount_sats > 0),
    address_user TEXT NOT NULL CHECK (length(address_user) BETWEEN 1 AND 64),
    credit_pool TEXT NOT NULL CHECK (credit_pool = 'herd'),
    reason TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 256),
    ledger_entry_id INTEGER NOT NULL UNIQUE REFERENCES ledger_entries(id),
    event_seq INTEGER NOT NULL UNIQUE REFERENCES event_log(seq),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
