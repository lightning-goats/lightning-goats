CREATE TABLE overlay_identity (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    stream_id TEXT NOT NULL
);
