-- Explicit root-configured presentation delegation; no financial authority.
CREATE TABLE feeding_notice_policy(singleton INTEGER PRIMARY KEY CHECK(singleton=1),owner TEXT NOT NULL);
CREATE TABLE feeding_overlay_receipts(epoch TEXT NOT NULL,owner TEXT NOT NULL,identity TEXT NOT NULL,document_json TEXT NOT NULL,event_seq INTEGER NOT NULL UNIQUE REFERENCES event_log(seq),PRIMARY KEY(epoch,owner,identity));
CREATE TRIGGER immutable_feeding_overlay BEFORE UPDATE ON feeding_overlay_receipts BEGIN SELECT RAISE(ABORT,'feeding overlay receipt is immutable'); END;
CREATE TRIGGER retained_feeding_overlay BEFORE DELETE ON feeding_overlay_receipts BEGIN SELECT RAISE(ABORT,'feeding overlay receipt is retained'); END;
