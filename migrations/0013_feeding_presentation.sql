-- Advisory presentation cache only. Dispatch always reads live gateway safety.
CREATE TABLE feeding_presentation (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    paused INTEGER NOT NULL CHECK (paused IN (0,1)),
    reason TEXT NOT NULL CHECK (reason IN ('ready','override','remote_disabled','mode_disabled','unavailable')),
    threshold_sats INTEGER NOT NULL CHECK (threshold_sats > 0),
    observed_at INTEGER NOT NULL CHECK (observed_at >= 0)
);
