-- Read-only reconciliation must not share the finite invoice delivery budget.
CREATE TABLE cashu_treasury_inspections (
    intent_id TEXT PRIMARY KEY REFERENCES cashu_treasury_issuances(intent_id),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK(typeof(attempts)='integer' AND attempts BETWEEN 0 AND 6),
    next_attempt INTEGER NOT NULL CHECK(typeof(next_attempt)='integer' AND next_attempt>=0),
    delivery_closed INTEGER NOT NULL DEFAULT 0 CHECK(delivery_closed IN (0,1))
);
