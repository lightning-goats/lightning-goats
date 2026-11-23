WITH settled_sources AS (
    SELECT r.intent_id,r.ledger_id,r.input_sats,r.change_sats
    FROM cashu_treasury_source_results r
    JOIN cashu_treasury_completions c USING(intent_id)
    UNION ALL
    SELECT intent_id,ledger_id,input_sats,change_sats FROM breez_treasury_source_results
), settled_intents AS (
    SELECT intent_id FROM cashu_treasury_completions
    UNION ALL
    SELECT DISTINCT intent_id FROM breez_treasury_source_results
)
