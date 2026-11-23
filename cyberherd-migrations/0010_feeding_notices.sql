-- One receipt per confirmed feeding, but one message per original credit source.
CREATE TABLE feeding_announcements(owner TEXT NOT NULL,request_id TEXT NOT NULL,identity TEXT,event_seq INTEGER REFERENCES domain_events(seq),PRIMARY KEY(owner,request_id),FOREIGN KEY(owner,request_id) REFERENCES feeding_barriers(owner,request_id));
CREATE INDEX feeding_announcement_identity ON feeding_announcements(owner,identity);
CREATE TRIGGER immutable_feeding_announcement BEFORE UPDATE ON feeding_announcements BEGIN SELECT RAISE(ABORT,'feeding announcement decision is frozen'); END;
CREATE TRIGGER retained_feeding_announcement BEFORE DELETE ON feeding_announcements BEGIN SELECT RAISE(ABORT,'feeding announcement decision is retained'); END;
