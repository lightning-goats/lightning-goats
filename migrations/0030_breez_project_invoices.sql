CREATE TABLE breez_project_invoices (
    id TEXT PRIMARY KEY NOT NULL,
    intent TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    invoice TEXT UNIQUE,
    payment_hash TEXT UNIQUE CHECK(length(payment_hash)=64),
    CHECK ((invoice IS NULL) = (payment_hash IS NULL))
);
CREATE INDEX breez_invoice_creation ON breez_project_invoices(created_at);
CREATE TRIGGER breez_invoice_hash_conflict BEFORE UPDATE OF payment_hash ON breez_project_invoices
WHEN EXISTS (SELECT 1 FROM strike_receive_requests WHERE payment_hash=NEW.payment_hash)
BEGIN SELECT RAISE(ABORT,'invoice hash already bound to Strike'); END;
CREATE TRIGGER strike_invoice_hash_conflict BEFORE INSERT ON strike_receive_requests
WHEN EXISTS (SELECT 1 FROM breez_project_invoices WHERE payment_hash=NEW.payment_hash)
BEGIN SELECT RAISE(ABORT,'invoice hash already bound to Breez'); END;
ALTER TABLE breez_project_invoices ADD COLUMN next_poll INTEGER NOT NULL DEFAULT 0;
CREATE INDEX breez_invoice_poll ON breez_project_invoices(next_poll) WHERE invoice IS NOT NULL;

CREATE VIEW zap_invoice_terms AS
SELECT z.receive_request_id,'strike' AS provider,z.request_json,z.payee_pubkey,z.receipt_pubkey,
       r.invoice,r.amount_msat,r.address_user,r.credit_pool,r.description_hash
FROM zap_receive_context z JOIN strike_receive_requests r USING(receive_request_id)
UNION ALL
SELECT id,'breez',json_extract(intent,'$.description'),json_extract(intent,'$.payee_pubkey'),
       json_extract(intent,'$.receipt_pubkey'),invoice,json_extract(intent,'$.amount_sat')*1000,
       json_extract(intent,'$.user'),json_extract(intent,'$.credit_pool'),json_extract(intent,'$.description_hash')
FROM breez_project_invoices WHERE invoice IS NOT NULL AND json_extract(intent,'$.zap')=1;

-- Preserve outstanding leases, signatures and relay acknowledgements across the provider change.
ALTER TABLE zap_receipt_relays RENAME TO zap_receipt_relays_legacy;
ALTER TABLE zap_receipt_jobs RENAME TO zap_receipt_jobs_legacy;
DROP TRIGGER zap_receipt_identity_immutable;
DROP TRIGGER zap_receipt_signed_immutable;
CREATE TABLE zap_receipt_jobs (
    settlement_id INTEGER PRIMARY KEY REFERENCES settled_payments(id),
    source_event_seq INTEGER NOT NULL UNIQUE REFERENCES event_log(seq),
    receive_request_id TEXT NOT NULL,
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

INSERT INTO zap_receipt_jobs SELECT * FROM zap_receipt_jobs_legacy;
INSERT INTO zap_receipt_relays SELECT * FROM zap_receipt_relays_legacy;
DROP TABLE zap_receipt_relays_legacy;
DROP TABLE zap_receipt_jobs_legacy;
CREATE TRIGGER zap_receipt_requires_invoice BEFORE INSERT ON zap_receipt_jobs
WHEN NOT EXISTS (
    SELECT 1 FROM zap_invoice_terms z JOIN settled_payments p
      ON p.id=NEW.settlement_id AND p.source=z.provider
    WHERE z.receive_request_id=NEW.receive_request_id
)
BEGIN SELECT RAISE(ABORT,'zap receipt requires issued invoice'); END;
CREATE TRIGGER breez_invoice_id_conflict BEFORE INSERT ON breez_project_invoices
WHEN EXISTS (SELECT 1 FROM strike_receive_requests WHERE receive_request_id=NEW.id)
BEGIN SELECT RAISE(ABORT,'invoice identity already bound to Strike'); END;
CREATE TRIGGER strike_invoice_id_conflict BEFORE INSERT ON strike_receive_requests
WHEN EXISTS (SELECT 1 FROM breez_project_invoices WHERE id=NEW.receive_request_id)
BEGIN SELECT RAISE(ABORT,'invoice identity already bound to Breez'); END;
CREATE TABLE breez_collected_claims (
    request_id TEXT PRIMARY KEY NOT NULL REFERENCES breez_project_invoices(id),
    proof_digest TEXT UNIQUE NOT NULL CHECK(length(proof_digest)=64 AND proof_digest NOT GLOB '*[^0-9a-f]*'),
    verified_at INTEGER NOT NULL DEFAULT(unixepoch())
);
CREATE TRIGGER breez_claim_no_update BEFORE UPDATE ON breez_collected_claims
BEGIN SELECT RAISE(ABORT,'collected claim evidence is immutable'); END;
CREATE TRIGGER breez_claim_no_delete BEFORE DELETE ON breez_collected_claims
BEGIN SELECT RAISE(ABORT,'collected claim evidence is retained'); END;
