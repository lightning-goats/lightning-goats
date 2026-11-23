-- First-observation order, shared by authenticated core and social input. This
-- is not the core paid ledger and grants no spending or feeder capability.
CREATE TABLE core_streams (
    owner TEXT PRIMARY KEY REFERENCES owners(owner),
    binding_json TEXT NOT NULL,
    received_through INTEGER NOT NULL CHECK(received_through>=0)
);
CREATE TABLE ingress (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    owner TEXT NOT NULL REFERENCES owners(owner),
    source_key TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('social','payment','feeding')),
    payload_json TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','done')),
    was_deferred INTEGER NOT NULL DEFAULT 0 CHECK(was_deferred IN (0,1)),
    result_json TEXT,
    UNIQUE(owner,source_key)
);
CREATE INDEX ingress_head ON ingress(owner,state,seq);
CREATE TABLE relay_windows (
    owner TEXT NOT NULL REFERENCES owners(owner),
    query_key TEXT NOT NULL,
    scanned_through INTEGER NOT NULL CHECK(scanned_through>=0),
    PRIMARY KEY(owner,query_key)
);
CREATE TABLE core_envelopes (
    owner TEXT NOT NULL REFERENCES owners(owner),
    epoch TEXT NOT NULL,
    core_seq INTEGER NOT NULL CHECK(core_seq>0),
    envelope_hash TEXT NOT NULL,
    PRIMARY KEY(owner,epoch,core_seq)
);
CREATE TABLE core_sources (
    owner TEXT NOT NULL REFERENCES owners(owner),
    source_key TEXT NOT NULL,
    proof_hash TEXT NOT NULL,
    PRIMARY KEY(owner,source_key)
);
CREATE TABLE feeding_barriers (
    owner TEXT NOT NULL REFERENCES owners(owner),
    request_id TEXT NOT NULL,
    ingress_seq INTEGER NOT NULL UNIQUE REFERENCES ingress(seq),
    feeding_json TEXT NOT NULL,
    snapshot_json TEXT NOT NULL,
    PRIMARY KEY(owner,request_id)
);
CREATE TRIGGER immutable_core_binding BEFORE UPDATE OF binding_json ON core_streams
BEGIN SELECT RAISE(ABORT,'core binding requires explicit recovery'); END;
CREATE TRIGGER immutable_core_envelope BEFORE UPDATE ON core_envelopes
BEGIN SELECT RAISE(ABORT,'immutable core envelope'); END;
CREATE TRIGGER immutable_core_source BEFORE UPDATE ON core_sources
BEGIN SELECT RAISE(ABORT,'immutable core source'); END;
CREATE TRIGGER immutable_ingress BEFORE UPDATE OF owner,source_key,kind,payload_json ON ingress
BEGIN SELECT RAISE(ABORT,'immutable ordered input'); END;
CREATE TRIGGER immutable_feeding_barrier BEFORE UPDATE ON feeding_barriers
BEGIN SELECT RAISE(ABORT,'immutable feeding membership barrier'); END;
-- Existing signed inbox work keeps its completed state. Its historical order
-- predates core activation; do not recreate already applied interactions.
INSERT INTO ingress(owner,source_key,kind,payload_json,state)
SELECT s.owner,'nostr:'||s.event_id,'social',json_quote(s.event_id),s.state
FROM social_inbox s JOIN observed_events e ON s.owner=e.owner AND s.event_id=e.event_id
ORDER BY e.created_at,e.event_id,s.owner;
