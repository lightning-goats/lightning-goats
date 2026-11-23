CREATE TABLE payout_cost_acks(grant_id TEXT NOT NULL,member TEXT NOT NULL,id TEXT NOT NULL UNIQUE,document_json TEXT NOT NULL,gross_sats INTEGER NOT NULL CHECK(typeof(gross_sats)='integer' AND gross_sats>0),spent_sats INTEGER NOT NULL CHECK(typeof(spent_sats)='integer' AND spent_sats>=0 AND spent_sats<=gross_sats),accepted_at INTEGER NOT NULL CHECK(accepted_at>=0),PRIMARY KEY(grant_id,member),FOREIGN KEY(grant_id,member) REFERENCES payout_rows(grant_id,member));
CREATE TRIGGER immutable_payout_cost_ack BEFORE UPDATE ON payout_cost_acks BEGIN SELECT RAISE(ABORT,'core failed cost receipt is immutable'); END;
CREATE TRIGGER retained_payout_cost_ack BEFORE DELETE ON payout_cost_acks BEGIN SELECT RAISE(ABORT,'core failed cost receipt is retained'); END;
CREATE TRIGGER cost_ack_requires_final_hold BEFORE INSERT ON payout_cost_acks
WHEN NOT EXISTS(SELECT 1 FROM payout_outcome_acks a JOIN payout_rows r ON r.grant_id=a.grant_id WHERE a.grant_id=NEW.grant_id AND r.member=NEW.member AND r.gross_sats=NEW.gross_sats AND a.held_sats>=NEW.gross_sats)
BEGIN SELECT RAISE(ABORT,'cost acknowledgement requires a retained final hold'); END;
