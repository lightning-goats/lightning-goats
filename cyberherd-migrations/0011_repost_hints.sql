CREATE TABLE delivery_repost_hints(owner TEXT NOT NULL,cycle INTEGER NOT NULL CHECK(cycle>0),member TEXT NOT NULL,identity TEXT NOT NULL,PRIMARY KEY(owner,cycle,member),UNIQUE(owner,identity),FOREIGN KEY(owner,identity) REFERENCES delivery_messages(owner,identity) DEFERRABLE INITIALLY DEFERRED);
CREATE TRIGGER immutable_repost_hint BEFORE UPDATE ON delivery_repost_hints BEGIN SELECT RAISE(ABORT,'repost hint is frozen'); END;
CREATE TRIGGER retained_repost_hint BEFORE DELETE ON delivery_repost_hints BEGIN SELECT RAISE(ABORT,'repost hint is retained'); END;
