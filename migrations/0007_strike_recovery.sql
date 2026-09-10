CREATE TABLE strike_inbox (
    work_key TEXT PRIMARY KEY,
    event_id TEXT NOT NULL,
    receive_request_id TEXT NOT NULL,
    receive_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','quarantined','done')),
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt INTEGER NOT NULL DEFAULT (unixepoch()),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    completed_at INTEGER
);
CREATE INDEX strike_inbox_due ON strike_inbox(status,next_attempt);
CREATE TABLE strike_recovery_scan (
    receive_request_id TEXT PRIMARY KEY REFERENCES strike_receive_requests(receive_request_id),
    page_offset INTEGER NOT NULL DEFAULT 0,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt INTEGER NOT NULL DEFAULT (unixepoch())
);
