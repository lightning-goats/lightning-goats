CREATE TABLE observed_events (
    owner TEXT NOT NULL REFERENCES owners(owner),
    event_id TEXT NOT NULL,
    kind INTEGER NOT NULL CHECK(kind BETWEEN 0 AND 65535),
    author TEXT NOT NULL,
    created_at INTEGER NOT NULL CHECK(created_at>=0),
    event_json TEXT NOT NULL,
    PRIMARY KEY(owner,event_id)
);
CREATE INDEX observed_target_time ON observed_events(owner,kind,created_at DESC,event_id);
CREATE INDEX observed_author_time ON observed_events(owner,author,kind,created_at DESC,event_id);
CREATE TABLE announcement_candidates (
    owner TEXT NOT NULL,
    policy_key TEXT NOT NULL,
    event_id TEXT NOT NULL,
    goal_id TEXT,
    PRIMARY KEY(owner,policy_key,event_id),
    FOREIGN KEY(owner,event_id) REFERENCES observed_events(owner,event_id)
);
CREATE INDEX announcement_goal ON announcement_candidates(owner,policy_key,goal_id);

CREATE TABLE stream_versions (
    owner TEXT NOT NULL,
    event_id TEXT NOT NULL,
    coordinate TEXT NOT NULL,
    authority_json TEXT NOT NULL,
    PRIMARY KEY(owner,event_id),
    FOREIGN KEY(owner,event_id) REFERENCES observed_events(owner,event_id)
);
CREATE INDEX stream_alias_lookup ON stream_versions(owner,coordinate,event_id);
CREATE TABLE stream_heads (
    owner TEXT NOT NULL,
    coordinate TEXT NOT NULL,
    event_id TEXT NOT NULL,
    PRIMARY KEY(owner,coordinate),
    FOREIGN KEY(owner,event_id) REFERENCES stream_versions(owner,event_id)
);
CREATE TABLE profile_heads (
    owner TEXT NOT NULL,
    pubkey TEXT NOT NULL,
    event_id TEXT NOT NULL,
    PRIMARY KEY(owner,pubkey),
    FOREIGN KEY(owner,event_id) REFERENCES observed_events(owner,event_id)
);

CREATE TABLE social_inbox (
    owner TEXT NOT NULL,
    event_id TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','done')),
    next_attempt INTEGER NOT NULL DEFAULT 0 CHECK(next_attempt>=0),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts>=0),
    PRIMARY KEY(owner,event_id),
    FOREIGN KEY(owner,event_id) REFERENCES observed_events(owner,event_id)
);
CREATE INDEX social_inbox_due ON social_inbox(owner,state,next_attempt,event_id);
