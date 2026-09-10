CREATE TABLE invoice_admissions (
    id TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    expires_at INTEGER NOT NULL DEFAULT (unixepoch()+30),
    finished INTEGER NOT NULL DEFAULT 0 CHECK(finished IN (0,1))
);
CREATE INDEX invoice_admissions_window ON invoice_admissions(created_at);
CREATE INDEX strike_receive_requests_created ON strike_receive_requests(created_at);
