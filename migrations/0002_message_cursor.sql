CREATE TABLE IF NOT EXISTS message_cursor (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    last_event_seq INTEGER NOT NULL CHECK (last_event_seq >= 0),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

INSERT OR IGNORE INTO message_cursor (singleton, last_event_seq) VALUES (1, 0);

ALTER TABLE message_outbox ADD COLUMN source_event_seq INTEGER;

CREATE UNIQUE INDEX IF NOT EXISTS message_outbox_source_event_seq_unique
ON message_outbox(source_event_seq)
WHERE source_event_seq IS NOT NULL;

CREATE INDEX IF NOT EXISTS message_outbox_status_created_idx
ON message_outbox(status, created_at);
