-- One immutable collector acknowledgment per retained receipt. Existing receipt
-- and feed rows are untouched, including historical monetary evidence.
CREATE TABLE collector_handoff_amounts (
    ledger_id INTEGER PRIMARY KEY REFERENCES retained_cashu_lots(ledger_id),
    collected_sats INTEGER NOT NULL CHECK(typeof(collected_sats)='integer' AND collected_sats>0)
);
CREATE TRIGGER bounded_collector_handoff BEFORE INSERT ON collector_handoff_amounts
WHEN NOT EXISTS (SELECT 1 FROM cashu_receipt_inbox WHERE ledger_entry_id=NEW.ledger_id AND net_sats>=NEW.collected_sats)
BEGIN SELECT RAISE(ABORT,'collector acknowledgment exceeds receipt'); END;
CREATE TRIGGER immutable_collector_handoff BEFORE UPDATE ON collector_handoff_amounts
BEGIN SELECT RAISE(ABORT,'immutable collector acknowledgment'); END;
CREATE TRIGGER retain_collector_handoff BEFORE DELETE ON collector_handoff_amounts
BEGIN SELECT RAISE(ABORT,'retained collector acknowledgment'); END;
CREATE TABLE feed_acquisition_proceeds (
    request_id TEXT NOT NULL,
    ledger_id INTEGER NOT NULL REFERENCES collector_handoff_amounts(ledger_id),
    sats INTEGER NOT NULL CHECK(typeof(sats)='integer' AND sats>=0),
    PRIMARY KEY(request_id,ledger_id),
    FOREIGN KEY(request_id,ledger_id) REFERENCES feed_reward_funding_parts(request_id,ledger_id)
);
CREATE TRIGGER immutable_acquisition_proceeds BEFORE UPDATE ON feed_acquisition_proceeds
BEGIN SELECT RAISE(ABORT,'immutable acquisition proceeds'); END;
CREATE TRIGGER retain_acquisition_proceeds BEFORE DELETE ON feed_acquisition_proceeds
BEGIN SELECT RAISE(ABORT,'retained acquisition proceeds'); END;
CREATE TRIGGER frozen_acquisition_proceeds BEFORE INSERT ON feed_acquisition_proceeds
WHEN EXISTS(SELECT 1 FROM feed_reward_funding WHERE request_id=NEW.request_id)
BEGIN SELECT RAISE(ABORT,'confirmed acquisition proceeds already frozen'); END;
