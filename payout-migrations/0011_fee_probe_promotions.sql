-- Promotion retains the exact final invoice and quote identities. A discovery
-- quote is not an executable quote merely because its monetary total fits.
CREATE TABLE payout_fee_promotions (
    grant_id TEXT NOT NULL,
    member TEXT NOT NULL,
    invoice_id TEXT NOT NULL UNIQUE REFERENCES payout_fee_probe_intents(id),
    quote_id TEXT NOT NULL UNIQUE REFERENCES payout_fee_probe_quotes(id),
    provider_quote_id TEXT NOT NULL UNIQUE REFERENCES payout_fee_observations(provider_quote_id),
    created_at INTEGER NOT NULL CHECK(created_at>=0),
    PRIMARY KEY(grant_id,member),
    FOREIGN KEY(grant_id,member) REFERENCES payout_rows(grant_id,member)
) STRICT;
CREATE TRIGGER bind_fee_promotion BEFORE INSERT ON payout_fee_promotions
WHEN NOT EXISTS (
 SELECT 1 FROM payout_fee_probe_intents i
 JOIN payout_fee_probe_quotes q ON q.invoice_id=i.id
 JOIN payout_fee_observations o ON o.grant_id=i.grant_id AND o.member=i.member AND o.round=i.round
 JOIN payout_invoice_intents ei ON ei.id=i.id AND ei.grant_id=i.grant_id AND ei.member=i.member
 JOIN payout_quote_intents eq ON eq.id=q.id AND eq.invoice_id=i.id
 JOIN payout_quote_records er ON er.intent_id=q.id AND er.provider_quote_id=o.provider_quote_id
 WHERE i.grant_id=NEW.grant_id AND i.member=NEW.member AND i.id=NEW.invoice_id
 AND q.id=NEW.quote_id AND o.provider_quote_id=NEW.provider_quote_id
 AND o.invoice_net_sats=i.net_sats AND o.total_sats<=o.gross_sats
 AND NEW.created_at>=o.observed_at
) OR EXISTS(SELECT 1 FROM payout_execute_intents WHERE intent_id=NEW.quote_id)
BEGIN SELECT RAISE(ABORT,'fee promotion requires the exact fitting unexecuted probe'); END;
CREATE TRIGGER immutable_fee_promotion BEFORE UPDATE ON payout_fee_promotions
BEGIN SELECT RAISE(ABORT,'promoted fee identities are frozen'); END;
CREATE TRIGGER retained_fee_promotion BEFORE DELETE ON payout_fee_promotions
BEGIN SELECT RAISE(ABORT,'promoted fee identities are retained'); END;
CREATE TABLE payout_fee_probe_duplicates (
    quote_id TEXT PRIMARY KEY REFERENCES payout_fee_probe_quotes(id),
    provider_quote_id TEXT NOT NULL,
    created_at INTEGER NOT NULL CHECK(created_at>=0)
) STRICT;
CREATE TRIGGER immutable_fee_probe_duplicate BEFORE UPDATE ON payout_fee_probe_duplicates
BEGIN SELECT RAISE(ABORT,'original fee quote identity is frozen'); END;
CREATE TRIGGER retained_fee_probe_duplicate BEFORE DELETE ON payout_fee_probe_duplicates
BEGIN SELECT RAISE(ABORT,'original fee quote identity is retained'); END;
