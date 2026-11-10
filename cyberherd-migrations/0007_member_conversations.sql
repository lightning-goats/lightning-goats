-- Current admission context; older generations remain in immutable events,
-- feeding snapshots and message records. Rejoining starts a fresh branch.
CREATE TABLE member_conversations (
    owner TEXT NOT NULL REFERENCES owners(owner),
    cycle INTEGER NOT NULL CHECK(cycle>0),
    member TEXT NOT NULL,
    conversation_json TEXT NOT NULL,
    PRIMARY KEY(owner,cycle,member)
);
ALTER TABLE feeding_barriers ADD COLUMN conversations_json TEXT NOT NULL DEFAULT '{}';
