CREATE TABLE cyberherd_overlay_receipts(epoch TEXT NOT NULL,owner TEXT NOT NULL,identity TEXT NOT NULL,document_json TEXT NOT NULL,event_seq INTEGER NOT NULL UNIQUE REFERENCES event_log(seq),PRIMARY KEY(epoch,owner,identity));
CREATE TRIGGER immutable_cyberherd_overlay BEFORE UPDATE ON cyberherd_overlay_receipts BEGIN SELECT RAISE(ABORT,'member overlay receipt is immutable'); END;
CREATE TRIGGER retained_cyberherd_overlay BEFORE DELETE ON cyberherd_overlay_receipts BEGIN SELECT RAISE(ABORT,'member overlay receipt is retained'); END;
