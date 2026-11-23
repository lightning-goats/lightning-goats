CREATE TABLE roster_overlay_receipts(epoch TEXT NOT NULL,owner TEXT NOT NULL,identity TEXT NOT NULL,document_json TEXT NOT NULL,event_seq INTEGER NOT NULL UNIQUE REFERENCES event_log(seq),PRIMARY KEY(epoch,owner,identity));
CREATE TABLE roster_overlay_state(singleton INTEGER PRIMARY KEY CHECK(singleton=1),owner TEXT NOT NULL,cycle INTEGER NOT NULL,revision INTEGER NOT NULL,document_json TEXT NOT NULL,event_seq INTEGER NOT NULL REFERENCES event_log(seq));
CREATE TRIGGER immutable_roster_receipt BEFORE UPDATE ON roster_overlay_receipts BEGIN SELECT RAISE(ABORT,'roster receipt is immutable'); END;
CREATE TRIGGER retained_roster_receipt BEFORE DELETE ON roster_overlay_receipts BEGIN SELECT RAISE(ABORT,'roster receipt is retained'); END;
