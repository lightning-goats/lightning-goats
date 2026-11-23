-- First acceptance of the authenticated wallet result. Store it separately
-- so existing receipts, provider evidence and report digests stay immutable.
CREATE TABLE cashu_treasury_fee_observations (
    intent_id TEXT PRIMARY KEY REFERENCES cashu_treasury_wallet_results(intent_id),
    observed_at INTEGER NOT NULL CHECK(typeof(observed_at)='integer' AND observed_at>=0)
);

-- Historical reports do not carry an acceptance time. Give them one full
-- rolling window from this database's migration, even after replay or restart.
INSERT INTO cashu_treasury_fee_observations(intent_id,observed_at)
SELECT r.intent_id,
       MAX(unixepoch(),i.created_at,COALESCE(d.verified_at,0),
           COALESCE((SELECT last_seen FROM cashu_treasury_clock WHERE singleton=1),0))
FROM cashu_treasury_wallet_results r
JOIN cashu_treasury_intents i ON i.id=r.intent_id
LEFT JOIN cashu_treasury_deposits d ON d.intent_id=r.intent_id;

CREATE TRIGGER cashu_treasury_fee_observations_immutable
BEFORE UPDATE ON cashu_treasury_fee_observations
BEGIN SELECT RAISE(ABORT,'Cashu treasury fee observation is immutable'); END;

CREATE TRIGGER cashu_treasury_fee_observations_retained
BEFORE DELETE ON cashu_treasury_fee_observations
BEGIN SELECT RAISE(ABORT,'Cashu treasury fee observation is retained'); END;
