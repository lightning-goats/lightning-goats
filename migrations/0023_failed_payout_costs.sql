-- Explicit protected operator cost attestations. Original failures/finals stay frozen.
CREATE TABLE payout_failed_costs(reservation_id TEXT NOT NULL,member TEXT NOT NULL,id TEXT NOT NULL UNIQUE,document_json TEXT NOT NULL,gross_sats INTEGER NOT NULL CHECK(typeof(gross_sats)='integer' AND gross_sats>0),spent_sats INTEGER NOT NULL CHECK(typeof(spent_sats)='integer' AND spent_sats>=0 AND spent_sats<=gross_sats),verified_at INTEGER NOT NULL CHECK(verified_at>=0),PRIMARY KEY(reservation_id,member),FOREIGN KEY(reservation_id,member) REFERENCES reward_outcome_rows(reservation_id,member));
CREATE TRIGGER immutable_failed_cost BEFORE UPDATE ON payout_failed_costs BEGIN SELECT RAISE(ABORT,'failed cost review is immutable'); END;
CREATE TRIGGER retained_failed_cost BEFORE DELETE ON payout_failed_costs BEGIN SELECT RAISE(ABORT,'failed cost review is retained'); END;
CREATE TRIGGER failed_cost_requires_final_hold BEFORE INSERT ON payout_failed_costs
WHEN NOT EXISTS(SELECT 1 FROM reward_outcome_rows r JOIN reward_outcome_finals f ON f.reservation_id=r.reservation_id WHERE r.reservation_id=NEW.reservation_id AND r.member=NEW.member AND r.gross_sats=NEW.gross_sats AND r.held_sats=NEW.gross_sats AND r.spent_sats=0 AND json_extract(r.row_json,'$.disposition.state')='FAILED')
BEGIN SELECT RAISE(ABORT,'cost review requires an original finalized failed hold'); END;
