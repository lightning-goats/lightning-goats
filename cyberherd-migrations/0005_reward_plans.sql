-- Private planning state only. No core credit, provider action or public post.
CREATE TABLE reward_profile_reads (
    owner TEXT NOT NULL REFERENCES owners(owner),
    pubkey TEXT NOT NULL,
    head_id TEXT,
    observed_at INTEGER NOT NULL CHECK(observed_at>=0),
    available INTEGER NOT NULL CHECK(available IN (0,1)),
    PRIMARY KEY(owner,pubkey),
    FOREIGN KEY(owner,head_id) REFERENCES observed_events(owner,event_id)
);
CREATE TABLE reward_plans (
    owner TEXT NOT NULL,
    request_id TEXT NOT NULL,
    plan_hash TEXT NOT NULL,
    plan_json TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('awaiting_reservation','reserved','no_payout')),
    reservation_json TEXT,
    PRIMARY KEY(owner,request_id),
    FOREIGN KEY(owner,request_id) REFERENCES feeding_barriers(owner,request_id),
    CHECK((state='reserved')=(reservation_json IS NOT NULL))
);
CREATE TRIGGER immutable_reward_plan BEFORE UPDATE ON reward_plans
WHEN NEW.owner IS NOT OLD.owner OR NEW.request_id IS NOT OLD.request_id
 OR NEW.plan_hash IS NOT OLD.plan_hash OR NEW.plan_json IS NOT OLD.plan_json
 OR OLD.state!='awaiting_reservation' OR NEW.state!='reserved'
BEGIN SELECT RAISE(ABORT,'reward plan is frozen; only reservation attachment is allowed'); END;
CREATE TRIGGER retained_reward_plan BEFORE DELETE ON reward_plans
BEGIN SELECT RAISE(ABORT,'reward plans and outcomes are retained'); END;

-- Staging GET results lets bounded rounds recover without refetching an entire
-- large roster. A signed address change invalidates this entry at freeze time.
CREATE TABLE reward_recipient_reads (
    owner TEXT NOT NULL,
    request_id TEXT NOT NULL,
    pubkey TEXT NOT NULL,
    head_id TEXT NOT NULL,
    address TEXT NOT NULL,
    recipient_json TEXT,
    PRIMARY KEY(owner,request_id,pubkey),
    FOREIGN KEY(owner,request_id) REFERENCES feeding_barriers(owner,request_id),
    FOREIGN KEY(owner,head_id) REFERENCES observed_events(owner,event_id)
);
