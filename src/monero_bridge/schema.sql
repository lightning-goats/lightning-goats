CREATE TABLE IF NOT EXISTS monero_bridge_meta (
 id INTEGER PRIMARY KEY CHECK(id=1), namespace TEXT NOT NULL,
 generation TEXT NOT NULL, clock INTEGER NOT NULL CHECK(clock>=0)
);
CREATE TABLE IF NOT EXISTS monero_bridge_intents (
 id TEXT PRIMARY KEY, request TEXT NOT NULL, reserved_at INTEGER NOT NULL,
 callback_token TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'creating'
   CHECK(state IN ('creating','bound','creation_unknown')),
 created TEXT, address TEXT UNIQUE, snapshot TEXT NOT NULL DEFAULT '[]',
 observed_at INTEGER, wallet_height INTEGER NOT NULL DEFAULT 0,
 revision INTEGER NOT NULL DEFAULT 0 CHECK(typeof(revision)='integer' AND revision>=0),
 available INTEGER NOT NULL DEFAULT 0 CHECK(available IN(0,1)), hold TEXT,
 last_attempt INTEGER NOT NULL DEFAULT 0, dirty INTEGER NOT NULL DEFAULT 0,
 lease TEXT NOT NULL DEFAULT '', lease_until INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS monero_bridge_poll ON monero_bridge_intents(state,hold,last_attempt);
CREATE INDEX IF NOT EXISTS monero_bridge_creation ON monero_bridge_intents(reserved_at);
CREATE TRIGGER IF NOT EXISTS monero_bridge_no_delete BEFORE DELETE ON monero_bridge_intents
 BEGIN SELECT RAISE(ABORT,'bridge replay history is retained'); END;
CREATE TRIGGER IF NOT EXISTS monero_bridge_identity BEFORE UPDATE ON monero_bridge_intents
 WHEN NEW.id IS NOT OLD.id OR NEW.request IS NOT OLD.request OR NEW.callback_token IS NOT OLD.callback_token
   OR NEW.reserved_at IS NOT OLD.reserved_at OR (OLD.address IS NOT NULL AND NEW.address IS NOT OLD.address)
   OR (OLD.created IS NOT NULL AND NEW.created IS NOT OLD.created)
   OR (OLD.hold IS NOT NULL AND NEW.hold IS NOT OLD.hold)
 BEGIN SELECT RAISE(ABORT,'immutable bridge binding or hold'); END;
