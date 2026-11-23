-- Private acquired-ecash provenance. No existing paid history is reinterpreted.
-- Runtime import remains disabled until protected policy explicitly registers
-- the wallet/core epochs and limits and funding history is initialized.
CREATE TABLE cashu_receipt_inbox (
    wallet_instance TEXT NOT NULL,
    acquisition_id TEXT NOT NULL,
    wallet_epoch TEXT NOT NULL,
    core_epoch TEXT NOT NULL,
    mint_id TEXT NOT NULL,
    evidence_digest TEXT NOT NULL CHECK(length(evidence_digest)=64),
    receipt_digest TEXT NOT NULL CHECK(length(receipt_digest)=64),
    receipt_json TEXT NOT NULL CHECK(json_valid(receipt_json) AND length(CAST(receipt_json AS BLOB))<=8192),
    net_sats INTEGER NOT NULL CHECK(typeof(net_sats)='integer' AND net_sats>0),
    ledger_entry_id INTEGER NOT NULL UNIQUE REFERENCES ledger_entries(id),
    event_seq INTEGER NOT NULL UNIQUE REFERENCES event_log(seq),
    original_feed_credit_sats INTEGER NOT NULL CHECK(typeof(original_feed_credit_sats)='integer' AND original_feed_credit_sats>=0),
    accepted_at INTEGER NOT NULL CHECK(accepted_at>=0),
    PRIMARY KEY(wallet_instance,acquisition_id),
    UNIQUE(wallet_instance,mint_id,evidence_digest)
);
CREATE INDEX cashu_receipt_rate ON cashu_receipt_inbox(accepted_at,net_sats);
CREATE TABLE cashu_import_clock (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    last_seen INTEGER NOT NULL CHECK(last_seen>=0)
);
CREATE TRIGGER cashu_receipt_immutable BEFORE UPDATE ON cashu_receipt_inbox
BEGIN SELECT RAISE(ABORT,'Cashu receipt is immutable'); END;
CREATE TRIGGER cashu_receipt_retained BEFORE DELETE ON cashu_receipt_inbox
BEGIN SELECT RAISE(ABORT,'Cashu receipt history is retained'); END;
CREATE TRIGGER cashu_clock_monotone BEFORE UPDATE ON cashu_import_clock
WHEN NEW.singleton IS NOT OLD.singleton OR NEW.last_seen<OLD.last_seen
BEGIN SELECT RAISE(ABORT,'Cashu import clock cannot regress'); END;
CREATE TRIGGER cashu_clock_retained BEFORE DELETE ON cashu_import_clock
BEGIN SELECT RAISE(ABORT,'Cashu import clock is retained'); END;

-- Freeze the exact sources selected for a physical attempt. Cashu source
-- selection is gated separately from aggregate credit. Treasury waiting never
-- occupies the physical attempt's globally serialized state.
CREATE TABLE feed_source_reservations (
    request_id TEXT PRIMARY KEY REFERENCES feed_attempts(id),
    threshold_sats INTEGER NOT NULL CHECK(typeof(threshold_sats)='integer' AND threshold_sats>0)
);
CREATE TABLE feed_source_parts (
    request_id TEXT NOT NULL REFERENCES feed_source_reservations(request_id),
    ledger_id INTEGER NOT NULL REFERENCES reward_funding_lots(ledger_id),
    sats INTEGER NOT NULL CHECK(typeof(sats)='integer' AND sats>0),
    PRIMARY KEY(request_id,ledger_id)
);
CREATE TRIGGER feed_source_reservation_immutable BEFORE UPDATE ON feed_source_reservations
BEGIN SELECT RAISE(ABORT,'feed source reservation is immutable'); END;
CREATE TRIGGER feed_source_reservation_retained BEFORE DELETE ON feed_source_reservations
BEGIN SELECT RAISE(ABORT,'feed source reservation is retained'); END;
CREATE TRIGGER feed_source_part_immutable BEFORE UPDATE ON feed_source_parts
BEGIN SELECT RAISE(ABORT,'feed source allocation is immutable'); END;
CREATE TRIGGER feed_source_part_retained BEFORE DELETE ON feed_source_parts
BEGIN SELECT RAISE(ABORT,'feed source allocation is retained'); END;

-- Treasury purpose and custody reservations precede invoice creation. Immutable
-- append-only boundaries distinguish a lost reply from permission to retry POST.
CREATE TABLE cashu_treasury_intents (
    id TEXT PRIMARY KEY,
    intent_digest TEXT NOT NULL UNIQUE CHECK(length(intent_digest)=64),
    intent_json TEXT NOT NULL CHECK(json_valid(intent_json) AND length(CAST(intent_json AS BLOB))<=65536),
    mint_id TEXT NOT NULL,
    input_sats INTEGER NOT NULL CHECK(typeof(input_sats)='integer' AND input_sats>0),
    deposit_sats INTEGER NOT NULL CHECK(typeof(deposit_sats)='integer' AND deposit_sats>0 AND deposit_sats<=input_sats),
    created_at INTEGER NOT NULL CHECK(created_at>=0)
);
CREATE TABLE cashu_treasury_sources (
    intent_id TEXT NOT NULL REFERENCES cashu_treasury_intents(id),
    ledger_id INTEGER NOT NULL REFERENCES cashu_receipt_inbox(ledger_entry_id),
    input_sats INTEGER NOT NULL CHECK(typeof(input_sats)='integer' AND input_sats>0),
    PRIMARY KEY(intent_id,ledger_id)
);
CREATE TABLE cashu_treasury_claims (
    intent_id TEXT PRIMARY KEY REFERENCES cashu_treasury_intents(id),
    claim_id TEXT NOT NULL UNIQUE,
    claimed_at INTEGER NOT NULL CHECK(claimed_at>=0)
);
CREATE TABLE cashu_treasury_issuances (
    intent_id TEXT PRIMARY KEY REFERENCES cashu_treasury_claims(intent_id),
    receive_request_id TEXT NOT NULL UNIQUE REFERENCES strike_receive_requests(receive_request_id),
    payment_hash TEXT NOT NULL UNIQUE CHECK(length(payment_hash)=64)
);
CREATE TABLE cashu_treasury_deposits (
    intent_id TEXT PRIMARY KEY REFERENCES cashu_treasury_issuances(intent_id),
    receive_id TEXT NOT NULL UNIQUE,
    payment_hash TEXT NOT NULL UNIQUE CHECK(length(payment_hash)=64),
    deposit_sats INTEGER NOT NULL CHECK(typeof(deposit_sats)='integer' AND deposit_sats>0),
    evidence_json TEXT NOT NULL CHECK(json_valid(evidence_json) AND length(CAST(evidence_json AS BLOB))<=32768),
    verified_at INTEGER NOT NULL CHECK(verified_at>=0)
);
CREATE TABLE cashu_treasury_wallet_results (
    intent_id TEXT PRIMARY KEY REFERENCES cashu_treasury_issuances(intent_id),
    result_digest TEXT NOT NULL CHECK(length(result_digest)=64),
    result_json TEXT NOT NULL CHECK(json_valid(result_json) AND length(CAST(result_json AS BLOB))<=65536)
);
CREATE TABLE cashu_treasury_completions (
    intent_id TEXT PRIMARY KEY REFERENCES cashu_treasury_deposits(intent_id),
    input_sats INTEGER NOT NULL CHECK(typeof(input_sats)='integer' AND input_sats>0),
    deposit_sats INTEGER NOT NULL CHECK(typeof(deposit_sats)='integer' AND deposit_sats>0),
    fee_sats INTEGER NOT NULL CHECK(typeof(fee_sats)='integer' AND fee_sats>=0),
    change_sats INTEGER NOT NULL CHECK(typeof(change_sats)='integer' AND change_sats>=0),
    CHECK(input_sats=deposit_sats+fee_sats+change_sats)
);
CREATE TABLE cashu_treasury_source_results (
    intent_id TEXT NOT NULL REFERENCES cashu_treasury_completions(intent_id) DEFERRABLE INITIALLY DEFERRED,
    ledger_id INTEGER NOT NULL,
    input_sats INTEGER NOT NULL CHECK(typeof(input_sats)='integer' AND input_sats>0),
    deposit_sats INTEGER NOT NULL CHECK(typeof(deposit_sats)='integer' AND deposit_sats>=0),
    fee_sats INTEGER NOT NULL CHECK(typeof(fee_sats)='integer' AND fee_sats>=0),
    change_sats INTEGER NOT NULL CHECK(typeof(change_sats)='integer' AND change_sats>=0),
    PRIMARY KEY(intent_id,ledger_id),
    FOREIGN KEY(intent_id,ledger_id) REFERENCES cashu_treasury_sources(intent_id,ledger_id),
    CHECK(input_sats=deposit_sats+fee_sats+change_sats)
);
CREATE TABLE cashu_treasury_clock (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    last_seen INTEGER NOT NULL CHECK(last_seen>=0)
);
CREATE TRIGGER cashu_treasury_clock_monotone BEFORE UPDATE ON cashu_treasury_clock
WHEN NEW.singleton IS NOT OLD.singleton OR NEW.last_seen<OLD.last_seen
BEGIN SELECT RAISE(ABORT,'treasury clock cannot regress'); END;
CREATE TRIGGER cashu_treasury_clock_retained BEFORE DELETE ON cashu_treasury_clock
BEGIN SELECT RAISE(ABORT,'treasury clock is retained'); END;

CREATE TRIGGER cashu_treasury_intents_immutable BEFORE UPDATE ON cashu_treasury_intents
BEGIN SELECT RAISE(ABORT,'treasury history is immutable'); END;
CREATE TRIGGER cashu_treasury_intents_retained BEFORE DELETE ON cashu_treasury_intents
BEGIN SELECT RAISE(ABORT,'treasury history is retained'); END;

CREATE TRIGGER cashu_treasury_sources_immutable BEFORE UPDATE ON cashu_treasury_sources
BEGIN SELECT RAISE(ABORT,'treasury history is immutable'); END;
CREATE TRIGGER cashu_treasury_sources_retained BEFORE DELETE ON cashu_treasury_sources
BEGIN SELECT RAISE(ABORT,'treasury history is retained'); END;

CREATE TRIGGER cashu_treasury_claims_immutable BEFORE UPDATE ON cashu_treasury_claims
BEGIN SELECT RAISE(ABORT,'treasury history is immutable'); END;
CREATE TRIGGER cashu_treasury_claims_retained BEFORE DELETE ON cashu_treasury_claims
BEGIN SELECT RAISE(ABORT,'treasury history is retained'); END;

CREATE TRIGGER cashu_treasury_issuances_immutable BEFORE UPDATE ON cashu_treasury_issuances
BEGIN SELECT RAISE(ABORT,'treasury history is immutable'); END;
CREATE TRIGGER cashu_treasury_issuances_retained BEFORE DELETE ON cashu_treasury_issuances
BEGIN SELECT RAISE(ABORT,'treasury history is retained'); END;

CREATE TRIGGER cashu_treasury_deposits_immutable BEFORE UPDATE ON cashu_treasury_deposits
BEGIN SELECT RAISE(ABORT,'treasury history is immutable'); END;
CREATE TRIGGER cashu_treasury_deposits_retained BEFORE DELETE ON cashu_treasury_deposits
BEGIN SELECT RAISE(ABORT,'treasury history is retained'); END;

CREATE TRIGGER cashu_treasury_wallet_results_immutable BEFORE UPDATE ON cashu_treasury_wallet_results
BEGIN SELECT RAISE(ABORT,'treasury history is immutable'); END;
CREATE TRIGGER cashu_treasury_wallet_results_retained BEFORE DELETE ON cashu_treasury_wallet_results
BEGIN SELECT RAISE(ABORT,'treasury history is retained'); END;

CREATE TRIGGER cashu_treasury_completions_immutable BEFORE UPDATE ON cashu_treasury_completions
BEGIN SELECT RAISE(ABORT,'treasury history is immutable'); END;
CREATE TRIGGER cashu_treasury_completions_retained BEFORE DELETE ON cashu_treasury_completions
BEGIN SELECT RAISE(ABORT,'treasury history is retained'); END;

CREATE TRIGGER cashu_treasury_source_results_immutable BEFORE UPDATE ON cashu_treasury_source_results
BEGIN SELECT RAISE(ABORT,'treasury history is immutable'); END;
CREATE TRIGGER cashu_treasury_source_results_retained BEFORE DELETE ON cashu_treasury_source_results
BEGIN SELECT RAISE(ABORT,'treasury history is retained'); END;

-- Bounded core work claims; immutable financial history above remains authoritative.
CREATE TABLE cashu_treasury_work (
    intent_id TEXT NOT NULL REFERENCES cashu_treasury_intents(id),
    stage TEXT NOT NULL CHECK(stage IN ('issue','wallet')),
    attempts INTEGER NOT NULL CHECK(typeof(attempts)='integer' AND attempts>0),
    next_attempt INTEGER NOT NULL CHECK(typeof(next_attempt)='integer' AND next_attempt>=0),
    PRIMARY KEY(intent_id,stage)
);

-- Financial plans precede physical attempts and never debit credit themselves.
CREATE TABLE feed_financial_plans (
    id TEXT PRIMARY KEY,
    threshold_sats INTEGER NOT NULL CHECK(typeof(threshold_sats)='integer' AND threshold_sats>0),
    created_at INTEGER NOT NULL DEFAULT(unixepoch()) CHECK(created_at>=0),
    state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','dispatched','released')),
    request_id TEXT UNIQUE REFERENCES feed_attempts(id),
    disposition TEXT,
    CHECK((state='pending' AND request_id IS NULL AND disposition IS NULL)
       OR (state='dispatched' AND request_id IS NOT NULL AND disposition='physical_intent')
       OR (state='released' AND request_id IS NULL AND disposition IN ('settled_supersession','threshold_changed','operator_release')))
);
CREATE UNIQUE INDEX one_pending_financial_plan ON feed_financial_plans((1)) WHERE state='pending';
CREATE TABLE feed_financial_parts (
    plan_id TEXT NOT NULL REFERENCES feed_financial_plans(id),
    ledger_id INTEGER NOT NULL REFERENCES reward_funding_lots(ledger_id),
    sats INTEGER NOT NULL CHECK(typeof(sats)='integer' AND sats>0),
    PRIMARY KEY(plan_id,ledger_id)
);
CREATE INDEX feed_financial_source ON feed_financial_parts(ledger_id,plan_id);
CREATE TRIGGER feed_financial_plan_transition BEFORE UPDATE ON feed_financial_plans
WHEN NEW.id IS NOT OLD.id OR NEW.threshold_sats IS NOT OLD.threshold_sats
 OR NEW.created_at IS NOT OLD.created_at OR OLD.state!='pending' OR NEW.state='pending'
BEGIN SELECT RAISE(ABORT,'invalid financial plan transition'); END;
CREATE TRIGGER feed_financial_plan_retained BEFORE DELETE ON feed_financial_plans
BEGIN SELECT RAISE(ABORT,'financial plan history is retained'); END;
CREATE TRIGGER feed_financial_part_immutable BEFORE UPDATE ON feed_financial_parts
BEGIN SELECT RAISE(ABORT,'financial allocation is immutable'); END;
CREATE TRIGGER feed_financial_part_retained BEFORE DELETE ON feed_financial_parts
BEGIN SELECT RAISE(ABORT,'financial allocation is retained'); END;

-- A job's source and custody reservation commit with its financial-plan link.
CREATE TABLE feed_financial_treasury (
    intent_id TEXT PRIMARY KEY REFERENCES cashu_treasury_intents(id),
    plan_id TEXT NOT NULL REFERENCES feed_financial_plans(id)
);
CREATE INDEX feed_financial_jobs ON feed_financial_treasury(plan_id,intent_id);
CREATE TRIGGER feed_financial_job_pending BEFORE INSERT ON feed_financial_treasury
WHEN NOT EXISTS(SELECT 1 FROM feed_financial_plans WHERE id=NEW.plan_id AND state='pending')
BEGIN SELECT RAISE(ABORT,'treasury job requires a pending financial plan'); END;
CREATE TRIGGER feed_financial_job_immutable BEFORE UPDATE ON feed_financial_treasury
BEGIN SELECT RAISE(ABORT,'financial treasury link is immutable'); END;
CREATE TRIGGER feed_financial_job_retained BEFORE DELETE ON feed_financial_treasury
BEGIN SELECT RAISE(ABORT,'financial treasury link is retained'); END;

CREATE TABLE feed_financial_supersessions (
    original_id TEXT PRIMARY KEY REFERENCES feed_financial_plans(id),
    replacement_id TEXT NOT NULL UNIQUE REFERENCES feed_financial_plans(id),
    CHECK(original_id!=replacement_id)
);
CREATE TRIGGER feed_financial_supersession_states BEFORE INSERT ON feed_financial_supersessions
WHEN NOT EXISTS(SELECT 1 FROM feed_financial_plans WHERE id=NEW.original_id AND state='released' AND disposition='settled_supersession')
 OR NOT EXISTS(SELECT 1 FROM feed_financial_plans WHERE id=NEW.replacement_id AND state='pending')
BEGIN SELECT RAISE(ABORT,'invalid financial supersession'); END;
CREATE TRIGGER feed_financial_supersession_immutable BEFORE UPDATE ON feed_financial_supersessions
BEGIN SELECT RAISE(ABORT,'financial supersession is immutable'); END;
CREATE TRIGGER feed_financial_supersession_retained BEFORE DELETE ON feed_financial_supersessions
BEGIN SELECT RAISE(ABORT,'financial supersession is retained'); END;

-- Source lookup remains indexed as immutable transfer history grows.
CREATE INDEX cashu_treasury_mint_admission ON cashu_treasury_intents(mint_id);
CREATE INDEX cashu_treasury_source_ledger ON cashu_treasury_sources(ledger_id,intent_id);
CREATE INDEX cashu_treasury_result_ledger ON cashu_treasury_source_results(ledger_id,intent_id);
CREATE INDEX cashu_receipt_custody_scope ON cashu_receipt_inbox(wallet_instance,wallet_epoch,core_epoch,mint_id,ledger_entry_id);

CREATE TABLE cashu_sweep_requests (
    id TEXT PRIMARY KEY,
    request_json TEXT NOT NULL CHECK(json_valid(request_json) AND length(CAST(request_json AS BLOB))<=4096),
    created_at INTEGER NOT NULL DEFAULT(unixepoch()) CHECK(created_at>=0)
);
CREATE TABLE cashu_sweep_jobs (
    intent_id TEXT PRIMARY KEY REFERENCES cashu_treasury_intents(id),
    reason TEXT NOT NULL CHECK(reason IN ('age','balance','operator')),
    request_id TEXT UNIQUE REFERENCES cashu_sweep_requests(id),
    trigger_json TEXT NOT NULL CHECK(json_valid(trigger_json) AND length(CAST(trigger_json AS BLOB))<=4096),
    CHECK((reason='operator' AND request_id IS NOT NULL) OR (reason!='operator' AND request_id IS NULL))
);
CREATE TRIGGER cashu_sweep_request_immutable BEFORE UPDATE ON cashu_sweep_requests
BEGIN SELECT RAISE(ABORT,'operator sweep request is immutable'); END;
CREATE TRIGGER cashu_sweep_request_retained BEFORE DELETE ON cashu_sweep_requests
BEGIN SELECT RAISE(ABORT,'operator sweep request is retained'); END;
CREATE TRIGGER cashu_sweep_job_immutable BEFORE UPDATE ON cashu_sweep_jobs
BEGIN SELECT RAISE(ABORT,'sweep job history is immutable'); END;
CREATE TRIGGER cashu_sweep_job_retained BEFORE DELETE ON cashu_sweep_jobs
BEGIN SELECT RAISE(ABORT,'sweep job history is retained'); END;
