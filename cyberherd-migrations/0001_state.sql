-- Separate CyberHerd state. This migration never opens or modifies the paid ledger.
CREATE TABLE owners (
    owner TEXT PRIMARY KEY,
    settings_json TEXT NOT NULL,
    roster_json TEXT NOT NULL,
    cycle INTEGER NOT NULL CHECK (cycle > 0),
    cycle_start INTEGER NOT NULL CHECK (cycle_start >= 0),
    revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0)
);
CREATE TABLE interactions (
    owner TEXT NOT NULL REFERENCES owners(owner),
    source_id TEXT NOT NULL,
    source_hash TEXT NOT NULL,
    input_json TEXT NOT NULL,
    occurred_at INTEGER NOT NULL CHECK (occurred_at >= 0),
    status TEXT NOT NULL CHECK (status IN ('deferred', 'applied', 'discarded')),
    result_json TEXT NOT NULL,
    PRIMARY KEY (owner, source_id)
);
CREATE INDEX deferred_interactions ON interactions(owner, status, occurred_at, source_id);
CREATE TABLE domain_events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    owner TEXT NOT NULL REFERENCES owners(owner),
    cycle INTEGER NOT NULL CHECK (cycle > 0),
    source_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    snapshot_json TEXT NOT NULL,
    UNIQUE (owner, source_id)
);
CREATE TABLE operator_commands (
    owner TEXT NOT NULL REFERENCES owners(owner),
    command_id TEXT NOT NULL,
    input_json TEXT NOT NULL,
    result_json TEXT NOT NULL,
    PRIMARY KEY (owner, command_id)
);
