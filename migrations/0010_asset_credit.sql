-- Additive receipt/valuation provenance. Never re-credit historical payments.
CREATE TABLE credit_valuations (
    id TEXT PRIMARY KEY NOT NULL,
    asset TEXT NOT NULL CHECK (asset IN ('BTC','XMR')),
    network TEXT NOT NULL,
    provider TEXT NOT NULL,
    account_scope TEXT NOT NULL,
    receive_scope TEXT NOT NULL,
    address_user TEXT NOT NULL,
    credit_pool TEXT NOT NULL,
    expected_atomic INTEGER NOT NULL CHECK (typeof(expected_atomic)='integer' AND expected_atomic>0),
    target_sats INTEGER NOT NULL CHECK (typeof(target_sats)='integer' AND target_sats>0),
    max_credit_sats INTEGER NOT NULL CHECK (typeof(max_credit_sats)='integer' AND max_credit_sats>=target_sats),
    -- Decimal strings preserve the full u64 rational without SQLite REAL coercion.
    rate_numerator TEXT,
    rate_denominator TEXT,
    rate_source TEXT,
    rate_observed_at INTEGER,
    issued_at INTEGER,
    expires_at INTEGER,
    policy_version TEXT NOT NULL,
    UNIQUE (asset,network,provider,account_scope,receive_scope),
    CHECK ((asset='BTC' AND expected_atomic=1 AND target_sats=1
            AND rate_numerator IS NULL AND rate_denominator IS NULL
            AND rate_source IS NULL AND rate_observed_at IS NULL
            AND issued_at IS NULL AND expires_at IS NULL)
        OR (asset='XMR' AND rate_numerator IS NOT NULL AND rate_denominator IS NOT NULL
            AND rate_source IS NOT NULL AND rate_observed_at IS NOT NULL
            AND issued_at IS NOT NULL AND expires_at IS NOT NULL AND rate_observed_at>=0
            AND issued_at>=rate_observed_at AND expires_at>issued_at))
);
CREATE TABLE credit_allocations (
    valuation_id TEXT PRIMARY KEY NOT NULL REFERENCES credit_valuations(id),
    eligible_atomic INTEGER NOT NULL DEFAULT 0 CHECK (typeof(eligible_atomic)='integer' AND eligible_atomic>=0),
    credited_sats INTEGER NOT NULL DEFAULT 0 CHECK (typeof(credited_sats)='integer' AND credited_sats>=0),
    hold_reason TEXT,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE TABLE asset_receipts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    valuation_id TEXT NOT NULL REFERENCES credit_valuations(id),
    receipt_key TEXT NOT NULL CHECK (length(receipt_key) BETWEEN 1 AND 256),
    amount_atomic INTEGER NOT NULL CHECK (typeof(amount_atomic)='integer' AND amount_atomic>0),
    first_seen_at INTEGER CHECK (first_seen_at IS NULL OR first_seen_at>=0),
    unlocked INTEGER NOT NULL CHECK (unlocked IN (0,1)),
    -- Eligibility is decided from immutable terms and first-seen evidence.
    eligible INTEGER NOT NULL CHECK (eligible IN (0,1)),
    btc_settlement_id INTEGER UNIQUE REFERENCES settled_payments(id),
    UNIQUE (valuation_id,receipt_key)
);
CREATE TABLE credit_grants (
    receipt_id INTEGER PRIMARY KEY REFERENCES asset_receipts(id),
    delta_sats INTEGER NOT NULL CHECK (typeof(delta_sats)='integer' AND delta_sats>=0),
    cumulative_atomic INTEGER NOT NULL CHECK (typeof(cumulative_atomic)='integer' AND cumulative_atomic>0),
    cumulative_sats INTEGER NOT NULL CHECK (typeof(cumulative_sats)='integer' AND cumulative_sats>=delta_sats),
    ledger_entry_id INTEGER UNIQUE REFERENCES ledger_entries(id),
    event_seq INTEGER UNIQUE REFERENCES event_log(seq),
    CHECK ((delta_sats=0 AND ledger_entry_id IS NULL AND event_seq IS NULL)
        OR (delta_sats>0 AND ledger_entry_id IS NOT NULL))
);
-- Private conflicting observations, deduplicated across retries. Not an outbox.
CREATE TABLE credit_conflicts (
    valuation_id TEXT NOT NULL REFERENCES credit_valuations(id),
    receipt_key TEXT NOT NULL,
    amount_atomic INTEGER NOT NULL,
    first_seen_at INTEGER NOT NULL,
    unlocked INTEGER NOT NULL,
    reason TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (valuation_id,receipt_key,amount_atomic,first_seen_at,unlocked,reason)
);

-- Fail the migration, rather than silently manufacture provenance for a corrupt
-- or missing old credit. Legacy rows without settled_payments are left untouched.
CREATE TABLE credit_migration_check (ok INTEGER NOT NULL CHECK (ok=1));
INSERT INTO credit_migration_check
SELECT CASE WHEN EXISTS (
    SELECT 1 FROM settled_payments p LEFT JOIN ledger_entries l
      ON l.source_key='payment:'||p.source||':'||p.source_id
    WHERE p.amount_msat%1000<>0 OR l.id IS NULL OR l.entry_type<>'HERD_RECEIPT'
       OR l.delta_sats<>p.amount_msat/1000 OR l.payment_source IS NOT p.source
       OR l.payment_source_id IS NOT p.source_id OR l.address_user IS NOT p.address_user
       OR l.credit_pool IS NOT p.credit_pool OR l.payment_hash IS NOT p.payment_hash
) THEN 0 ELSE 1 END;
DROP TABLE credit_migration_check;

-- The old SettledPayment contract did not carry network/account identity. Say
-- legacy-unspecified rather than inventing a mainnet/account attestation.
INSERT INTO credit_valuations
 (id,asset,network,provider,account_scope,receive_scope,address_user,credit_pool,
  expected_atomic,target_sats,max_credit_sats,policy_version)
SELECT 'btc:'||id,'BTC','legacy-unspecified',source,'legacy-settled-payment',source_id,
       address_user,credit_pool,1,1,9223372036854775807,'btc-identity-v1'
FROM settled_payments;
INSERT INTO credit_allocations (valuation_id,eligible_atomic,credited_sats)
SELECT 'btc:'||id,amount_msat/1000,amount_msat/1000 FROM settled_payments;
INSERT INTO asset_receipts
 (valuation_id,receipt_key,amount_atomic,first_seen_at,unlocked,eligible,btc_settlement_id)
SELECT 'btc:'||id,source_id,amount_msat/1000,NULL,1,1,id FROM settled_payments;
INSERT INTO credit_grants
 (receipt_id,delta_sats,cumulative_atomic,cumulative_sats,ledger_entry_id,event_seq)
SELECT r.id,p.amount_msat/1000,p.amount_msat/1000,p.amount_msat/1000,l.id,NULL
FROM settled_payments p JOIN asset_receipts r ON r.btc_settlement_id=p.id
JOIN ledger_entries l ON l.source_key='payment:'||p.source||':'||p.source_id;
-- Old event bytes/sequences are deliberately not reconstructed or rewritten.

CREATE TRIGGER credit_valuations_no_update BEFORE UPDATE ON credit_valuations
BEGIN SELECT RAISE(ABORT,'credit valuation is immutable'); END;
CREATE TRIGGER credit_valuations_no_delete BEFORE DELETE ON credit_valuations
BEGIN SELECT RAISE(ABORT,'credit valuation is immutable'); END;
CREATE TRIGGER asset_receipts_identity_immutable BEFORE UPDATE ON asset_receipts
WHEN NEW.id IS NOT OLD.id OR NEW.valuation_id IS NOT OLD.valuation_id
 OR NEW.receipt_key IS NOT OLD.receipt_key OR NEW.amount_atomic IS NOT OLD.amount_atomic
 OR NEW.first_seen_at IS NOT OLD.first_seen_at OR NEW.eligible IS NOT OLD.eligible
 OR NEW.btc_settlement_id IS NOT OLD.btc_settlement_id OR NEW.unlocked<OLD.unlocked
BEGIN SELECT RAISE(ABORT,'receipt identity or finality is immutable'); END;
CREATE TRIGGER asset_receipts_no_delete BEFORE DELETE ON asset_receipts
BEGIN SELECT RAISE(ABORT,'receipt history must be retained'); END;
CREATE TRIGGER credit_grants_no_update BEFORE UPDATE ON credit_grants
BEGIN SELECT RAISE(ABORT,'credit grant is immutable'); END;
CREATE TRIGGER credit_grants_no_delete BEFORE DELETE ON credit_grants
BEGIN SELECT RAISE(ABORT,'credit grant is immutable'); END;
CREATE TRIGGER credit_allocations_monotone BEFORE UPDATE ON credit_allocations
WHEN NEW.valuation_id IS NOT OLD.valuation_id OR NEW.eligible_atomic<OLD.eligible_atomic
 OR NEW.credited_sats<OLD.credited_sats
 OR (OLD.hold_reason IS NOT NULL AND NEW.hold_reason IS NOT OLD.hold_reason)
BEGIN SELECT RAISE(ABORT,'allocation cannot regress or release a hold'); END;
CREATE TRIGGER credit_allocations_no_delete BEFORE DELETE ON credit_allocations
BEGIN SELECT RAISE(ABORT,'allocation history must be retained'); END;
