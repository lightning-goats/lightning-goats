CREATE TABLE breez_treasury_wallet_results (
    intent_id TEXT PRIMARY KEY NOT NULL REFERENCES breez_project_invoices(id)
        REFERENCES cashu_treasury_intents(id),
    result_digest TEXT NOT NULL CHECK(length(result_digest)=64 AND result_digest NOT GLOB '*[^0-9a-f]*'),
    result_json TEXT NOT NULL CHECK(json_valid(result_json) AND length(CAST(result_json AS BLOB))<=65536),
    observed_at INTEGER NOT NULL CHECK(observed_at>=0)
);
CREATE TRIGGER immutable_breez_treasury_result BEFORE UPDATE ON breez_treasury_wallet_results
BEGIN SELECT RAISE(ABORT,'immutable conversion wallet result'); END;
CREATE TRIGGER retain_breez_treasury_result BEFORE DELETE ON breez_treasury_wallet_results
BEGIN SELECT RAISE(ABORT,'retained conversion wallet result'); END;

CREATE VIEW breez_treasury_source_results AS
SELECT w.intent_id,c.proof_digest,s.ledger_id,s.input_sats,
       json_extract(p.value,'$.deposit_sats') AS deposit_sats,
       json_extract(p.value,'$.fee_sats') AS fee_sats,
       json_extract(p.value,'$.change_sats') AS change_sats
FROM breez_treasury_wallet_results w
JOIN breez_collected_claims c ON c.request_id=w.intent_id
JOIN cashu_treasury_sources s ON s.intent_id=w.intent_id
JOIN json_each(w.result_json,'$.sources') p ON json_extract(p.value,'$.ledger_id')=s.ledger_id;
