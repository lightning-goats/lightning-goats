CREATE TABLE payout_core_execution_claims (
    grant_id TEXT NOT NULL,
    member TEXT NOT NULL,
    claim_id TEXT NOT NULL UNIQUE,
    document_json TEXT NOT NULL,
    PRIMARY KEY(grant_id,member),
    FOREIGN KEY(grant_id,member) REFERENCES payout_rows(grant_id,member)
);
CREATE TRIGGER immutable_core_execution_claim BEFORE UPDATE ON payout_core_execution_claims
BEGIN SELECT RAISE(ABORT,'core execution history is immutable'); END;
CREATE TRIGGER retained_core_execution_claim BEFORE DELETE ON payout_core_execution_claims
BEGIN SELECT RAISE(ABORT,'core execution history is retained'); END;
