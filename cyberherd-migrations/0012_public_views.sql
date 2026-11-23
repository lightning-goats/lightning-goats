CREATE TABLE view_discovery(owner TEXT PRIMARY KEY REFERENCES owners(owner),observed_at INTEGER NOT NULL CHECK(observed_at>=0),available INTEGER NOT NULL CHECK(available IN (0,1)));
CREATE TABLE roster_delivery(owner TEXT PRIMARY KEY REFERENCES owners(owner),document_json TEXT NOT NULL,event_seq INTEGER NOT NULL CHECK(event_seq>0));
