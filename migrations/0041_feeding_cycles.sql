-- The cycle journal is initialized against paid state by the core's startup
-- transaction. Installing the schema alone leaves the legacy writer behavior.
CREATE TABLE feed_cycle_policy (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 initialized_at INTEGER NOT NULL CHECK(initialized_at>=0),
 legacy_threshold_sats INTEGER NOT NULL CHECK(legacy_threshold_sats>0),
 legacy_through_ledger_id INTEGER NOT NULL CHECK(legacy_through_ledger_id>=0)
) STRICT;

CREATE TABLE feed_cycle_price (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 rate_usd TEXT NOT NULL,
 source TEXT NOT NULL CHECK(source='OpenHAB BTC_USD_Price'),
 observed_at_ms INTEGER NOT NULL CHECK(observed_at_ms>=0),
 target_sats INTEGER NOT NULL CHECK(target_sats>0)
) STRICT;

CREATE TABLE feed_cycles (
 seq INTEGER PRIMARY KEY,
 id TEXT NOT NULL UNIQUE,
 opened_at INTEGER NOT NULL CHECK(opened_at>=0),
 target_sats INTEGER NOT NULL CHECK(target_sats>0),
 rate_usd TEXT,
 source TEXT NOT NULL CHECK(source IN ('legacy','OpenHAB BTC_USD_Price')),
 observed_at_ms INTEGER,
 credited_sats INTEGER NOT NULL DEFAULT 0 CHECK(credited_sats BETWEEN 0 AND target_sats),
 funded_at INTEGER,
 confirmed_attempt TEXT UNIQUE REFERENCES feed_attempts(id),
 CHECK((source='legacy' AND rate_usd IS NULL AND observed_at_ms IS NULL)
    OR (source='OpenHAB BTC_USD_Price' AND rate_usd IS NOT NULL AND observed_at_ms IS NOT NULL)),
 CHECK((credited_sats=target_sats)=(funded_at IS NOT NULL)),
 CHECK(confirmed_attempt IS NULL OR funded_at IS NOT NULL)
) STRICT;
CREATE UNIQUE INDEX feed_one_open_cycle ON feed_cycles((1)) WHERE funded_at IS NULL;
CREATE INDEX feed_funded_cycle_queue ON feed_cycles(seq) WHERE funded_at IS NOT NULL AND confirmed_attempt IS NULL;

CREATE TABLE feed_cycle_pending (
 ledger_id INTEGER PRIMARY KEY REFERENCES ledger_entries(id),
 sats INTEGER NOT NULL CHECK(sats>0)
) STRICT;

CREATE TABLE feed_cycle_allocations (
 cycle_id TEXT NOT NULL REFERENCES feed_cycles(id),
 ledger_id INTEGER NOT NULL REFERENCES ledger_entries(id),
 sats INTEGER NOT NULL CHECK(sats>0),
 PRIMARY KEY(cycle_id, ledger_id)
) STRICT;
CREATE INDEX feed_cycle_contribution_history ON feed_cycle_allocations(ledger_id);

CREATE TABLE feed_cycle_attempts (
 attempt_id TEXT PRIMARY KEY REFERENCES feed_attempts(id),
 cycle_id TEXT NOT NULL REFERENCES feed_cycles(id),
 migrated_attempt INTEGER NOT NULL CHECK(migrated_attempt IN (0,1))
) STRICT;
CREATE INDEX feed_cycle_attempt_history ON feed_cycle_attempts(cycle_id);

CREATE TABLE feed_cycle_plans (
 plan_id TEXT PRIMARY KEY REFERENCES feed_financial_plans(id),
 cycle_id TEXT NOT NULL REFERENCES feed_cycles(id)
) STRICT;

CREATE TRIGGER feed_cycle_prices_immutable BEFORE UPDATE ON feed_cycles
WHEN NEW.id<>OLD.id OR NEW.seq<>OLD.seq OR NEW.opened_at<>OLD.opened_at
 OR NEW.target_sats<>OLD.target_sats OR NEW.rate_usd IS NOT OLD.rate_usd
 OR NEW.source<>OLD.source OR NEW.observed_at_ms IS NOT OLD.observed_at_ms
 OR NEW.credited_sats<OLD.credited_sats
 OR (OLD.funded_at IS NOT NULL AND NEW.funded_at IS NOT OLD.funded_at)
 OR (OLD.confirmed_attempt IS NOT NULL AND NEW.confirmed_attempt IS NOT OLD.confirmed_attempt)
BEGIN SELECT RAISE(ABORT,'feeding cycle terms and completed allocations are retained'); END;

CREATE TRIGGER feed_cycle_allocations_immutable BEFORE UPDATE ON feed_cycle_allocations
BEGIN SELECT RAISE(ABORT,'feeding cycle contribution history is immutable'); END;

CREATE TRIGGER feed_cycle_allocations_retained BEFORE DELETE ON feed_cycle_allocations
BEGIN SELECT RAISE(ABORT,'feeding cycle contribution history is retained'); END;
