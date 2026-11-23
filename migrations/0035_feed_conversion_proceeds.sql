CREATE TABLE feed_conversion_proceeds (
    request_id TEXT NOT NULL,
    ledger_id INTEGER NOT NULL,
    intent_id TEXT NOT NULL REFERENCES breez_collected_claims(request_id),
    sats INTEGER NOT NULL CHECK(typeof(sats)='integer' AND sats>0),
    PRIMARY KEY(request_id,ledger_id,intent_id),
    FOREIGN KEY(request_id,ledger_id) REFERENCES feed_reward_funding_parts(request_id,ledger_id),
    FOREIGN KEY(ledger_id,intent_id) REFERENCES cashu_funding_ranges(ledger_id,intent_id)
);
CREATE TRIGGER immutable_feed_conversion BEFORE UPDATE ON feed_conversion_proceeds
BEGIN SELECT RAISE(ABORT,'immutable conversion proceeds'); END;
CREATE TRIGGER retain_feed_conversion BEFORE DELETE ON feed_conversion_proceeds
BEGIN SELECT RAISE(ABORT,'retained conversion proceeds'); END;
CREATE TRIGGER frozen_feed_conversion BEFORE INSERT ON feed_conversion_proceeds
WHEN EXISTS(SELECT 1 FROM feed_reward_funding WHERE request_id=NEW.request_id)
BEGIN SELECT RAISE(ABORT,'confirmed conversion proceeds already frozen'); END;
