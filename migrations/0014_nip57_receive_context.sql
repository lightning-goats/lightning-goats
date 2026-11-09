CREATE TABLE zap_receive_context (
    receive_request_id TEXT PRIMARY KEY REFERENCES strike_receive_requests(receive_request_id),
    request_json TEXT NOT NULL CHECK (length(request_json) BETWEEN 1 AND 8192),
    payee_pubkey TEXT NOT NULL CHECK (length(payee_pubkey)=64),
    receipt_pubkey TEXT NOT NULL CHECK (length(receipt_pubkey)=64)
);
CREATE TRIGGER zap_context_no_update BEFORE UPDATE ON zap_receive_context
BEGIN SELECT RAISE(ABORT,'zap receive context is immutable'); END;
CREATE TRIGGER zap_context_no_delete BEFORE DELETE ON zap_receive_context
BEGIN SELECT RAISE(ABORT,'zap receive context is retained for replay protection'); END;

-- Independent protocol receipts; never share the kind-1 announcement identity.
CREATE TABLE zap_receipt_jobs (
    settlement_id INTEGER PRIMARY KEY REFERENCES settled_payments(id),
    source_event_seq INTEGER NOT NULL UNIQUE REFERENCES event_log(seq),
    receive_request_id TEXT NOT NULL REFERENCES zap_receive_context(receive_request_id),
    paid_at INTEGER NOT NULL CHECK (paid_at>=0),
    event_id TEXT UNIQUE CHECK (event_id IS NULL OR length(event_id)=64),
    signed_json TEXT,
    lease_id TEXT,
    lease_until INTEGER,
    next_attempt INTEGER NOT NULL DEFAULT 0 CHECK (next_attempt>=0),
    signing_attempts INTEGER NOT NULL DEFAULT 0 CHECK (signing_attempts>=0),
    CHECK ((event_id IS NULL AND signed_json IS NULL) OR (event_id IS NOT NULL AND signed_json IS NOT NULL))
);
CREATE TRIGGER zap_receipt_identity_immutable BEFORE UPDATE OF settlement_id,source_event_seq,receive_request_id,paid_at ON zap_receipt_jobs
BEGIN SELECT RAISE(ABORT,'zap receipt source identity is immutable'); END;
CREATE TRIGGER zap_receipt_signed_immutable BEFORE UPDATE OF event_id,signed_json ON zap_receipt_jobs
WHEN OLD.event_id IS NOT NULL
BEGIN SELECT RAISE(ABORT,'signed zap receipts cannot be replaced'); END;

CREATE TABLE zap_receipt_relays (
    settlement_id INTEGER NOT NULL REFERENCES zap_receipt_jobs(settlement_id),
    relay TEXT NOT NULL,
    published INTEGER NOT NULL DEFAULT 0 CHECK (published IN (0,1)),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts>=0),
    next_attempt INTEGER NOT NULL DEFAULT 0 CHECK (next_attempt>=0),
    PRIMARY KEY(settlement_id,relay)
);
