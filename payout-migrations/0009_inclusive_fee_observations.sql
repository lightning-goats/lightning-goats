-- Fee discovery is append-only evidence, never an execution capability.
-- Original grants, invoice intents, quotes and payment identities stay frozen.
CREATE TABLE payout_fee_observations (
    grant_id TEXT NOT NULL,
    member TEXT NOT NULL,
    round INTEGER NOT NULL CHECK(round BETWEEN 0 AND 7),
    provider_quote_id TEXT NOT NULL UNIQUE,
    gross_sats INTEGER NOT NULL CHECK(gross_sats>0),
    invoice_net_sats INTEGER NOT NULL CHECK(invoice_net_sats>0 AND invoice_net_sats<=gross_sats),
    fee_sats INTEGER NOT NULL CHECK(fee_sats>=0 AND fee_sats<=9223372036854775807-invoice_net_sats),
    total_sats INTEGER NOT NULL CHECK(total_sats=invoice_net_sats+fee_sats),
    observed_at INTEGER NOT NULL CHECK(observed_at>=0),
    document_json TEXT NOT NULL CHECK(length(CAST(document_json AS BLOB))<=65536),
    PRIMARY KEY(grant_id,member,round),
    FOREIGN KEY(grant_id,member) REFERENCES payout_rows(grant_id,member)
) STRICT;
CREATE TRIGGER bind_fee_observation BEFORE INSERT ON payout_fee_observations
WHEN NEW.gross_sats IS NOT (SELECT gross_sats FROM payout_rows WHERE grant_id=NEW.grant_id AND member=NEW.member)
 OR EXISTS(SELECT 1 FROM payout_invoice_intents i JOIN payout_quote_intents q ON q.invoice_id=i.id
           JOIN payout_execute_intents e ON e.intent_id=q.id
           WHERE i.grant_id=NEW.grant_id AND i.member=NEW.member)
 OR (NEW.round=0 AND (NEW.invoice_net_sats!=NEW.gross_sats
                     OR EXISTS(SELECT 1 FROM payout_fee_observations WHERE grant_id=NEW.grant_id AND member=NEW.member)))
 OR (NEW.round>0 AND NOT EXISTS(
     SELECT 1 FROM payout_fee_observations old WHERE old.grant_id=NEW.grant_id AND old.member=NEW.member
       AND old.round=NEW.round-1 AND old.total_sats>old.gross_sats
       AND old.fee_sats<old.gross_sats AND NEW.invoice_net_sats=old.gross_sats-old.fee_sats
       AND NEW.invoice_net_sats<old.invoice_net_sats AND NEW.observed_at>=old.observed_at))
BEGIN SELECT RAISE(ABORT,'fee observation must preserve gross and strictly reduce an unexecuted attempt'); END;
CREATE TRIGGER immutable_fee_observation BEFORE UPDATE ON payout_fee_observations
BEGIN SELECT RAISE(ABORT,'fee observations are immutable'); END;
CREATE TRIGGER retained_fee_observation BEFORE DELETE ON payout_fee_observations
BEGIN SELECT RAISE(ABORT,'fee observations are retained'); END;
