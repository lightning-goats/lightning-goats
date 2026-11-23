-- A receipt remains whole; unusable feeder credit is explicitly retired rather
-- than represented as food, a refund, a payout, or deleted paid history.
CREATE TABLE feeder_credit_policy (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 threshold_sats INTEGER NOT NULL CHECK(threshold_sats>0),
 max_feedings INTEGER NOT NULL CHECK(max_feedings=3)
) STRICT;
CREATE TABLE feed_credit_retirements (
 id TEXT PRIMARY KEY,
 credit_ledger_id INTEGER NOT NULL REFERENCES ledger_entries(id),
 debit_ledger_id INTEGER NOT NULL UNIQUE REFERENCES ledger_entries(id),
 retired_sats INTEGER NOT NULL CHECK(retired_sats>0),
 reason TEXT NOT NULL CHECK(reason IN ('three_feeding_cap','operator_backlog_correction')),
 event_seq INTEGER NOT NULL UNIQUE REFERENCES event_log(seq),
 created_at INTEGER NOT NULL DEFAULT(unixepoch())
) STRICT;
CREATE TRIGGER bind_feed_credit_retirement BEFORE INSERT ON feed_credit_retirements
WHEN NOT EXISTS(SELECT 1 FROM ledger_entries c,ledger_entries d
 WHERE c.id=NEW.credit_ledger_id AND c.delta_sats>0 AND d.id=NEW.debit_ledger_id
 AND d.entry_type='FEED_CREDIT_RETIRED' AND d.delta_sats=-NEW.retired_sats
 AND d.source_key='feed-credit-retirement:'||NEW.id AND d.feed_attempt_id IS NULL)
 OR NEW.retired_sats+COALESCE((SELECT SUM(retired_sats) FROM feed_credit_retirements WHERE credit_ledger_id=NEW.credit_ledger_id),0)
    >(SELECT delta_sats FROM ledger_entries WHERE id=NEW.credit_ledger_id)
BEGIN SELECT RAISE(ABORT,'retirement must retain its original credit and non-feeding audit debit'); END;
CREATE TRIGGER immutable_feed_credit_retirement BEFORE UPDATE ON feed_credit_retirements
BEGIN SELECT RAISE(ABORT,'retired credit history is immutable'); END;
CREATE TRIGGER retained_feed_credit_retirement BEFORE DELETE ON feed_credit_retirements
BEGIN SELECT RAISE(ABORT,'retired credit history is retained'); END;
CREATE TABLE feeding_announcement_sources (
 credit_ledger_id INTEGER PRIMARY KEY REFERENCES ledger_entries(id),
 first_event_seq INTEGER NOT NULL UNIQUE REFERENCES event_log(seq)
) STRICT;
CREATE TRIGGER immutable_feeding_announcement_source BEFORE UPDATE ON feeding_announcement_sources
BEGIN SELECT RAISE(ABORT,'first feeding announcement is immutable'); END;
CREATE TRIGGER retained_feeding_announcement_source BEFORE DELETE ON feeding_announcement_sources
BEGIN SELECT RAISE(ABORT,'first feeding announcement is retained'); END;
-- Retain first true confirmation for older receipts as well. Never rewrite
-- those events or their already signed/published Nostr bytes.
INSERT INTO feeding_announcement_sources(credit_ledger_id,first_event_seq)
SELECT json_extract(e.payload_json,'$.triggering_credit_id'),MIN(e.seq)
FROM feed_event_sources s JOIN event_log e ON e.seq=s.event_seq
JOIN ledger_entries l ON l.id=json_extract(e.payload_json,'$.triggering_credit_id')
WHERE e.event_type='feeder_confirmed' AND l.delta_sats>0
AND json_type(e.payload_json,'$.triggering_credit_id')='integer'
GROUP BY l.id;
CREATE TABLE message_outbox_suppressions (
 event_id TEXT PRIMARY KEY REFERENCES message_outbox(event_id),
 reason TEXT NOT NULL CHECK(reason='duplicate_feeding_announcement'),
 created_at INTEGER NOT NULL DEFAULT(unixepoch())
) STRICT;
INSERT INTO message_outbox_suppressions(event_id,reason)
SELECT o.event_id,'duplicate_feeding_announcement' FROM message_outbox o
JOIN event_log e ON e.seq=o.source_event_seq
JOIN feeding_announcement_sources f ON f.credit_ledger_id=json_extract(e.payload_json,'$.triggering_credit_id')
WHERE e.event_type='feeder_confirmed' AND o.status IN ('pending','failed') AND e.seq!=f.first_event_seq;
CREATE TRIGGER immutable_message_suppression BEFORE UPDATE ON message_outbox_suppressions
BEGIN SELECT RAISE(ABORT,'outbox suppression audit is immutable'); END;
CREATE TRIGGER retained_message_suppression BEFORE DELETE ON message_outbox_suppressions
BEGIN SELECT RAISE(ABORT,'outbox suppression audit is retained'); END;
