CREATE TABLE http_auth_nonces(id TEXT PRIMARY KEY,expires INTEGER NOT NULL CHECK(expires>=0));
CREATE INDEX http_auth_nonce_expiry ON http_auth_nonces(expires);
CREATE TABLE operator_http_audit(owner TEXT NOT NULL REFERENCES owners(owner),actor TEXT NOT NULL,command_id TEXT NOT NULL,event_id TEXT NOT NULL UNIQUE,body_hash TEXT NOT NULL,input_json TEXT NOT NULL,result_json TEXT NOT NULL,created_at INTEGER NOT NULL CHECK(created_at>=0),PRIMARY KEY(owner,actor,command_id));
CREATE TRIGGER immutable_http_operator_audit BEFORE UPDATE ON operator_http_audit BEGIN SELECT RAISE(ABORT,'operator audit is immutable'); END;
CREATE TRIGGER retained_http_operator_audit BEFORE DELETE ON operator_http_audit BEGIN SELECT RAISE(ABORT,'operator audit is retained'); END;
