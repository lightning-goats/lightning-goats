CREATE TABLE feeder_cooldown (
    singleton INTEGER PRIMARY KEY CHECK (singleton=1),
    resume_after INTEGER NOT NULL
);
INSERT INTO feeder_cooldown VALUES (1, 0);
