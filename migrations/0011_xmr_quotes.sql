-- Quote creation is NOT a payable address or an asset receipt. Keep immutable
-- requests (including failed/expired ones) so an old ID can never silently reprice.
CREATE TABLE xmr_quote_requests (
    id TEXT PRIMARY KEY,
    identity_json TEXT NOT NULL,
    client_bucket TEXT NOT NULL CHECK(length(client_bucket)=64),
    requested_at INTEGER NOT NULL CHECK(requested_at>=0),
    reservation_until INTEGER NOT NULL CHECK(reservation_until>requested_at),
    status TEXT NOT NULL CHECK(status IN ('reserved','ready','failed')),
    document_json TEXT,
    CHECK((status='ready' AND document_json IS NOT NULL) OR (status!='ready' AND document_json IS NULL))
);
CREATE INDEX xmr_quote_requests_time ON xmr_quote_requests(requested_at);
CREATE INDEX xmr_quote_requests_client_time ON xmr_quote_requests(client_bucket,requested_at);
CREATE INDEX xmr_quote_requests_pending ON xmr_quote_requests(status,reservation_until);
CREATE TABLE xmr_quote_clock (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    last_epoch INTEGER NOT NULL CHECK(last_epoch>=0)
);
INSERT INTO xmr_quote_clock VALUES(1,0);
CREATE TABLE xmr_rate_watermarks (
    source TEXT PRIMARY KEY,
    observed_at INTEGER NOT NULL CHECK(observed_at>=0),
    numerator TEXT NOT NULL,
    denominator TEXT NOT NULL
);
CREATE TABLE xmr_quote_bindings (
    quote_id TEXT PRIMARY KEY REFERENCES xmr_quote_requests(id),
    valuation_id TEXT NOT NULL UNIQUE REFERENCES credit_valuations(id),
    receive_scope TEXT NOT NULL,
    bound_at INTEGER NOT NULL CHECK(bound_at>=0)
);
CREATE TRIGGER xmr_quote_request_immutable BEFORE UPDATE ON xmr_quote_requests
WHEN OLD.status!='reserved' OR NEW.id IS NOT OLD.id
 OR NEW.identity_json IS NOT OLD.identity_json OR NEW.client_bucket IS NOT OLD.client_bucket
 OR NEW.requested_at IS NOT OLD.requested_at OR NEW.reservation_until IS NOT OLD.reservation_until
 OR NEW.status='reserved'
BEGIN SELECT RAISE(ABORT,'quote request is immutable after reservation'); END;
CREATE TRIGGER xmr_quote_request_no_delete BEFORE DELETE ON xmr_quote_requests
BEGIN SELECT RAISE(ABORT,'quote request replay history must be retained'); END;
CREATE TRIGGER xmr_quote_binding_no_update BEFORE UPDATE ON xmr_quote_bindings
BEGIN SELECT RAISE(ABORT,'quote binding is immutable'); END;
CREATE TRIGGER xmr_quote_binding_no_delete BEFORE DELETE ON xmr_quote_bindings
BEGIN SELECT RAISE(ABORT,'quote binding must be retained'); END;
CREATE TRIGGER xmr_quote_clock_no_regression BEFORE UPDATE ON xmr_quote_clock
WHEN NEW.singleton IS NOT OLD.singleton OR NEW.last_epoch<OLD.last_epoch
BEGIN SELECT RAISE(ABORT,'quote clock cannot regress'); END;
CREATE TRIGGER xmr_quote_clock_no_delete BEFORE DELETE ON xmr_quote_clock
BEGIN SELECT RAISE(ABORT,'quote clock must be retained'); END;
CREATE TRIGGER xmr_rate_no_regression BEFORE UPDATE ON xmr_rate_watermarks
WHEN NEW.source IS NOT OLD.source OR NEW.observed_at<OLD.observed_at
 OR (NEW.observed_at=OLD.observed_at AND (NEW.numerator IS NOT OLD.numerator OR NEW.denominator IS NOT OLD.denominator))
BEGIN SELECT RAISE(ABORT,'rate watermark cannot regress or change at the same timestamp'); END;
CREATE TRIGGER xmr_rate_no_delete BEFORE DELETE ON xmr_rate_watermarks
BEGIN SELECT RAISE(ABORT,'rate watermark must be retained'); END;
