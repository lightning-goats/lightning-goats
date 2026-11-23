-- Give verified settlements a permanent order. Fee change can be swept again
-- but can qualify the originating contribution for physical feeding only once.
CREATE TABLE cashu_settled_ranges (
    ledger_id INTEGER NOT NULL,
    intent_id TEXT NOT NULL,
    input_sats INTEGER NOT NULL CHECK(input_sats>0),
    deposit_sats INTEGER NOT NULL CHECK(deposit_sats BETWEEN 0 AND input_sats),
    start_sats INTEGER NOT NULL CHECK(start_sats>=0),
    PRIMARY KEY(ledger_id,intent_id),
    FOREIGN KEY(intent_id,ledger_id) REFERENCES cashu_treasury_source_results(intent_id,ledger_id)
);
INSERT INTO cashu_settled_ranges
SELECT r.ledger_id,r.intent_id,r.input_sats,r.deposit_sats,
    COALESCE(SUM(r.input_sats-r.change_sats) OVER (
        PARTITION BY r.ledger_id ORDER BY c.rowid
        ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING),0)
FROM cashu_treasury_source_results r JOIN cashu_treasury_completions c USING(intent_id);
CREATE TRIGGER freeze_cashu_settled_ranges AFTER INSERT ON cashu_treasury_completions
BEGIN
    INSERT INTO cashu_settled_ranges
    SELECT r.ledger_id,r.intent_id,r.input_sats,r.deposit_sats,
        (SELECT COALESCE(SUM(p.input_sats-p.change_sats),0) FROM cashu_treasury_source_results p
         JOIN cashu_treasury_completions c ON c.intent_id=p.intent_id
         WHERE p.ledger_id=r.ledger_id AND p.intent_id!=r.intent_id)
    FROM cashu_treasury_source_results r WHERE r.intent_id=NEW.intent_id;
END;

-- Only new confirmations freeze payout proceeds. Historical proofs are immutable.
CREATE TABLE feed_cashu_proceeds (
    request_id TEXT NOT NULL,
    ledger_id INTEGER NOT NULL,
    intent_id TEXT NOT NULL,
    sats INTEGER NOT NULL CHECK(typeof(sats)='integer' AND sats>0),
    PRIMARY KEY(request_id,ledger_id,intent_id),
    FOREIGN KEY(request_id,ledger_id) REFERENCES feed_reward_funding_parts(request_id,ledger_id),
    FOREIGN KEY(intent_id,ledger_id) REFERENCES cashu_treasury_source_results(intent_id,ledger_id)
);
CREATE TRIGGER feed_cashu_proceeds_immutable BEFORE UPDATE ON feed_cashu_proceeds
BEGIN SELECT RAISE(ABORT,'immutable feed settlement proceeds'); END;
CREATE TRIGGER feed_cashu_proceeds_retained BEFORE DELETE ON feed_cashu_proceeds
BEGIN SELECT RAISE(ABORT,'retained feed settlement proceeds'); END;
CREATE TRIGGER cashu_settled_ranges_immutable BEFORE UPDATE ON cashu_settled_ranges
BEGIN SELECT RAISE(ABORT,'immutable settlement order'); END;
CREATE TRIGGER cashu_settled_ranges_retained BEFORE DELETE ON cashu_settled_ranges
BEGIN SELECT RAISE(ABORT,'retained settlement order'); END;
CREATE TRIGGER feed_cashu_proceeds_frozen BEFORE INSERT ON feed_cashu_proceeds
WHEN EXISTS(SELECT 1 FROM feed_reward_funding WHERE request_id=NEW.request_id)
BEGIN SELECT RAISE(ABORT,'confirmed feed proceeds already frozen'); END;
