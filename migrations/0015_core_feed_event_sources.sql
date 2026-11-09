-- Additive source binding for new confirmed feed events. Historical events are
-- preserved, never guessed into reward authority from their JSON payload alone.
CREATE TABLE feed_event_sources (
    feed_attempt_id TEXT PRIMARY KEY REFERENCES feed_attempts(id),
    event_seq INTEGER NOT NULL UNIQUE REFERENCES event_log(seq)
);
CREATE TRIGGER feed_event_source_no_update BEFORE UPDATE ON feed_event_sources
BEGIN SELECT RAISE(ABORT,'feed event source is immutable'); END;
CREATE TRIGGER feed_event_source_no_delete BEFORE DELETE ON feed_event_sources
BEGIN SELECT RAISE(ABORT,'feed event source is retained'); END;
