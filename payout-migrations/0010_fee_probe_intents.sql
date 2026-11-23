-- Independent pre-execution discovery identities. The single legacy execution
-- invoice per grant/member remains unchanged. Only a final, matching attempt
-- may subsequently be promoted through its existing execution checks.
CREATE TABLE payout_fee_probe_intents (
    id TEXT PRIMARY KEY,
    grant_id TEXT NOT NULL,
    member TEXT NOT NULL,
    round INTEGER NOT NULL CHECK(round BETWEEN 0 AND 7),
    previous_quote_id TEXT REFERENCES payout_fee_observations(provider_quote_id),
    net_sats INTEGER NOT NULL CHECK(net_sats>0),
    document_json TEXT NOT NULL CHECK(length(CAST(document_json AS BLOB))<=65536),
    created_at INTEGER NOT NULL CHECK(created_at>=0),
    UNIQUE(grant_id,member,round),
    CHECK((round=0)=(previous_quote_id IS NULL)),
    FOREIGN KEY(grant_id,member) REFERENCES payout_rows(grant_id,member)
) STRICT;
CREATE TABLE payout_fee_probe_callbacks (
    intent_id TEXT PRIMARY KEY REFERENCES payout_fee_probe_intents(id),
    raw_zap TEXT,
    created_at INTEGER NOT NULL CHECK(created_at>=0)
) STRICT;
CREATE TABLE payout_fee_probe_invoices (
    intent_id TEXT PRIMARY KEY REFERENCES payout_fee_probe_callbacks(intent_id),
    payment_hash TEXT NOT NULL UNIQUE,
    document_json TEXT NOT NULL CHECK(length(CAST(document_json AS BLOB))<=32768),
    validated_at INTEGER NOT NULL CHECK(validated_at>=0)
) STRICT;
CREATE TABLE payout_fee_probe_quotes (
    id TEXT PRIMARY KEY,
    invoice_id TEXT NOT NULL UNIQUE REFERENCES payout_fee_probe_invoices(intent_id),
    document_json TEXT NOT NULL CHECK(length(CAST(document_json AS BLOB))<=32768)
) STRICT;
CREATE TRIGGER bind_fee_probe_intent BEFORE INSERT ON payout_fee_probe_intents
WHEN EXISTS(SELECT 1 FROM payout_invoice_intents WHERE grant_id=NEW.grant_id AND member=NEW.member)
 OR NEW.created_at<(SELECT admitted_at FROM payout_grants WHERE id=NEW.grant_id)
 OR (NEW.round=0 AND NEW.net_sats IS NOT (SELECT gross_sats FROM payout_rows WHERE grant_id=NEW.grant_id AND member=NEW.member))
 OR (NEW.round>0 AND NOT EXISTS(SELECT 1 FROM payout_fee_observations o
      WHERE o.provider_quote_id=NEW.previous_quote_id AND o.grant_id=NEW.grant_id AND o.member=NEW.member
        AND o.round=NEW.round-1 AND o.total_sats>o.gross_sats AND o.fee_sats<o.gross_sats
        AND NEW.net_sats=o.gross_sats-o.fee_sats AND NEW.net_sats<o.invoice_net_sats
        AND NEW.created_at>=o.observed_at))
BEGIN SELECT RAISE(ABORT,'fee probe must retain the original member budget and adjustment chain'); END;
CREATE TRIGGER immutable_fee_probe_intents BEFORE UPDATE ON payout_fee_probe_intents
BEGIN SELECT RAISE(ABORT,'fee probe identities are frozen'); END;
CREATE TRIGGER retained_fee_probe_intents BEFORE DELETE ON payout_fee_probe_intents
BEGIN SELECT RAISE(ABORT,'fee probe identities are retained'); END;
CREATE TRIGGER immutable_fee_probe_callbacks BEFORE UPDATE ON payout_fee_probe_callbacks
BEGIN SELECT RAISE(ABORT,'fee probe zap bytes are frozen'); END;
CREATE TRIGGER retained_fee_probe_callbacks BEFORE DELETE ON payout_fee_probe_callbacks
BEGIN SELECT RAISE(ABORT,'fee probe zap bytes are retained'); END;
CREATE TRIGGER immutable_fee_probe_invoices BEFORE UPDATE ON payout_fee_probe_invoices
BEGIN SELECT RAISE(ABORT,'fee probe invoices are frozen'); END;
CREATE TRIGGER retained_fee_probe_invoices BEFORE DELETE ON payout_fee_probe_invoices
BEGIN SELECT RAISE(ABORT,'fee probe invoices are retained'); END;
CREATE TRIGGER immutable_fee_probe_quotes BEFORE UPDATE ON payout_fee_probe_quotes
BEGIN SELECT RAISE(ABORT,'fee probe quote requests are frozen'); END;
CREATE TRIGGER retained_fee_probe_quotes BEFORE DELETE ON payout_fee_probe_quotes
BEGIN SELECT RAISE(ABORT,'fee probe quote requests are retained'); END;
