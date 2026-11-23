-- One settlement order per source, including change reused after a provider cutover.
CREATE TABLE cashu_funding_ranges (
    ledger_id INTEGER NOT NULL,
    intent_id TEXT NOT NULL,
    start_sats INTEGER NOT NULL CHECK(start_sats>=0),
    input_sats INTEGER NOT NULL CHECK(input_sats>0),
    deposit_sats INTEGER NOT NULL CHECK(deposit_sats BETWEEN 0 AND input_sats),
    change_sats INTEGER NOT NULL CHECK(change_sats BETWEEN 0 AND input_sats-deposit_sats),
    PRIMARY KEY(ledger_id,intent_id),
    FOREIGN KEY(intent_id,ledger_id) REFERENCES cashu_treasury_sources(intent_id,ledger_id)
);
INSERT INTO cashu_funding_ranges
SELECT g.ledger_id,g.intent_id,g.start_sats,g.input_sats,g.deposit_sats,r.change_sats
FROM cashu_settled_ranges g JOIN cashu_treasury_source_results r USING(ledger_id,intent_id);

INSERT INTO cashu_funding_ranges
SELECT r.ledger_id,r.intent_id,
    (SELECT COALESCE(SUM(input_sats-change_sats),0) FROM cashu_settled_ranges g
     JOIN cashu_treasury_source_results s USING(ledger_id,intent_id,input_sats)
     WHERE g.ledger_id=r.ledger_id)
    + COALESCE(SUM(r.input_sats-r.change_sats) OVER (
      PARTITION BY r.ledger_id ORDER BY MAX(w.observed_at,c.verified_at),r.intent_id
      ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING),0),
    r.input_sats,r.deposit_sats,r.change_sats
FROM breez_treasury_source_results r JOIN breez_treasury_wallet_results w USING(intent_id)
JOIN breez_collected_claims c ON c.request_id=r.intent_id;

CREATE TRIGGER freeze_legacy_funding_range AFTER INSERT ON cashu_settled_ranges
BEGIN
    INSERT INTO cashu_funding_ranges
    SELECT NEW.ledger_id,NEW.intent_id,
        (SELECT COALESCE(SUM(input_sats-change_sats),0) FROM cashu_funding_ranges WHERE ledger_id=NEW.ledger_id),
        NEW.input_sats,NEW.deposit_sats,r.change_sats FROM cashu_treasury_source_results r
    WHERE r.ledger_id=NEW.ledger_id AND r.intent_id=NEW.intent_id;
END;
CREATE TRIGGER freeze_conversion_range_on_wallet AFTER INSERT ON breez_treasury_wallet_results
BEGIN
    INSERT INTO cashu_funding_ranges
    SELECT r.ledger_id,r.intent_id,
        (SELECT COALESCE(SUM(input_sats-change_sats),0) FROM cashu_funding_ranges WHERE ledger_id=r.ledger_id),
        r.input_sats,r.deposit_sats,r.change_sats FROM breez_treasury_source_results r
    WHERE r.intent_id=NEW.intent_id;
END;
CREATE TRIGGER freeze_conversion_range_on_proof AFTER INSERT ON breez_collected_claims
BEGIN
    INSERT INTO cashu_funding_ranges
    SELECT r.ledger_id,r.intent_id,
        (SELECT COALESCE(SUM(input_sats-change_sats),0) FROM cashu_funding_ranges WHERE ledger_id=r.ledger_id),
        r.input_sats,r.deposit_sats,r.change_sats FROM breez_treasury_source_results r
    WHERE r.intent_id=NEW.request_id;
END;
CREATE TRIGGER immutable_cashu_funding_range BEFORE UPDATE ON cashu_funding_ranges
BEGIN SELECT RAISE(ABORT,'immutable funding range'); END;
CREATE TRIGGER retain_cashu_funding_range BEFORE DELETE ON cashu_funding_ranges
BEGIN SELECT RAISE(ABORT,'retained funding range'); END;
