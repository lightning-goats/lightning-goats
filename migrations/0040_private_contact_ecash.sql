-- Preserve original Lightning evidence and every delivery lease and relay ACK.
CREATE TABLE private_contact_entitlements (
    contact_id TEXT PRIMARY KEY REFERENCES private_contact_intents(id),
    rail TEXT NOT NULL CHECK(rail IN ('lightning','cashu')),
    receipt_id TEXT NOT NULL
) STRICT;
INSERT INTO private_contact_entitlements SELECT contact_id,'lightning',receive_id FROM private_contact_fees;

CREATE TABLE private_contact_requests (
    contact_id TEXT PRIMARY KEY REFERENCES private_contact_intents(id),
    request TEXT NOT NULL,
    descriptor_id TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT(unixepoch())
) STRICT;
CREATE TABLE private_contact_tenders (
    delivery_id TEXT PRIMARY KEY CHECK(length(delivery_id)=36),
    contact_id TEXT NOT NULL REFERENCES private_contact_requests(contact_id),
    state TEXT NOT NULL DEFAULT 'reserved' CHECK(state IN ('reserved','rejected','acquired')),
    created_at INTEGER NOT NULL DEFAULT(unixepoch())
) STRICT;
CREATE UNIQUE INDEX private_contact_pending_tender ON private_contact_tenders(contact_id)
WHERE state='reserved';
CREATE TABLE private_contact_cashu_receipts (
    acquisition_id TEXT PRIMARY KEY CHECK(length(acquisition_id)=36),
    delivery_id TEXT NOT NULL UNIQUE REFERENCES private_contact_tenders(delivery_id),
    wallet_instance TEXT NOT NULL,
    wallet_epoch TEXT NOT NULL,
    core_epoch TEXT NOT NULL,
    receipt_digest TEXT NOT NULL UNIQUE CHECK(length(receipt_digest)=64),
    evidence_digest TEXT NOT NULL,
    mint_id TEXT NOT NULL,
    net_sats INTEGER NOT NULL CHECK(net_sats>=100),
    receive_fee_sats INTEGER NOT NULL CHECK(receive_fee_sats>=0),
    accepted_at INTEGER NOT NULL DEFAULT(unixepoch()),
    UNIQUE(wallet_instance,evidence_digest)
) STRICT;

ALTER TABLE private_contact_delivery RENAME TO private_contact_delivery_v39;
CREATE TABLE private_contact_delivery (
    contact_id TEXT PRIMARY KEY REFERENCES private_contact_entitlements(contact_id),
    lease_id TEXT,
    lease_until INTEGER,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts>=0),
    next_attempt INTEGER NOT NULL DEFAULT(unixepoch()),
    accepted_relay TEXT,
    accepted_at INTEGER,
    CHECK((lease_id IS NULL)=(lease_until IS NULL)),
    CHECK((accepted_relay IS NULL)=(accepted_at IS NULL))
);
INSERT INTO private_contact_delivery(rowid,contact_id,lease_id,lease_until,attempts,next_attempt,accepted_relay,accepted_at)
SELECT rowid,contact_id,lease_id,lease_until,attempts,next_attempt,accepted_relay,accepted_at FROM private_contact_delivery_v39;
DROP TABLE private_contact_delivery_v39;
CREATE INDEX private_contact_delivery_due ON private_contact_delivery(accepted_at,next_attempt);
CREATE TRIGGER private_contact_delivery_monotone BEFORE UPDATE ON private_contact_delivery
WHEN NEW.contact_id IS NOT OLD.contact_id OR NEW.attempts<OLD.attempts
 OR (OLD.accepted_at IS NOT NULL AND (NEW.accepted_at IS NOT OLD.accepted_at OR NEW.accepted_relay IS NOT OLD.accepted_relay))
BEGIN SELECT RAISE(ABORT,'private delivery identity and acceptance are retained'); END;
CREATE TRIGGER private_contact_delivery_retained BEFORE DELETE ON private_contact_delivery
BEGIN SELECT RAISE(ABORT,'private delivery replay identity is retained'); END;

CREATE TRIGGER private_contact_lightning_entitlement AFTER INSERT ON private_contact_fees
BEGIN
    INSERT INTO private_contact_entitlements VALUES(NEW.contact_id,'lightning',NEW.receive_id) ON CONFLICT(contact_id) DO NOTHING;
END;
CREATE TRIGGER private_contact_cashu_entitlement AFTER INSERT ON private_contact_cashu_receipts
BEGIN
    INSERT INTO private_contact_entitlements
        SELECT contact_id,'cashu',NEW.acquisition_id FROM private_contact_tenders WHERE delivery_id=NEW.delivery_id
        ON CONFLICT(contact_id) DO NOTHING;
    INSERT INTO private_contact_delivery(contact_id)
        SELECT contact_id FROM private_contact_tenders WHERE delivery_id=NEW.delivery_id
        ON CONFLICT(contact_id) DO NOTHING;
    UPDATE private_contact_tenders SET state='acquired' WHERE delivery_id=NEW.delivery_id;
END;
CREATE TRIGGER private_contact_cashu_requires_reservation BEFORE INSERT ON private_contact_cashu_receipts
WHEN NOT EXISTS(SELECT 1 FROM private_contact_tenders WHERE delivery_id=NEW.delivery_id AND state='reserved')
BEGIN SELECT RAISE(ABORT,'private contact tender requires original reservation'); END;
CREATE TRIGGER private_contact_tender_transition BEFORE UPDATE ON private_contact_tenders
WHEN OLD.state!='reserved' OR NEW.state NOT IN ('rejected','acquired') OR NEW.delivery_id!=OLD.delivery_id
 OR NEW.contact_id!=OLD.contact_id OR NEW.created_at!=OLD.created_at
BEGIN SELECT RAISE(ABORT,'private contact tender is already bound'); END;
CREATE TRIGGER private_contact_tender_retained BEFORE DELETE ON private_contact_tenders
BEGIN SELECT RAISE(ABORT,'private contact tender is retained'); END;
CREATE TRIGGER private_contact_request_immutable BEFORE UPDATE ON private_contact_requests
BEGIN SELECT RAISE(ABORT,'private contact request is immutable'); END;
CREATE TRIGGER private_contact_request_retained BEFORE DELETE ON private_contact_requests
BEGIN SELECT RAISE(ABORT,'private contact request is retained'); END;
CREATE TRIGGER private_contact_cashu_receipt_immutable BEFORE UPDATE ON private_contact_cashu_receipts
BEGIN SELECT RAISE(ABORT,'private contact receipt is immutable'); END;
CREATE TRIGGER private_contact_cashu_receipt_retained BEFORE DELETE ON private_contact_cashu_receipts
BEGIN SELECT RAISE(ABORT,'private contact receipt is retained'); END;
CREATE TRIGGER private_contact_entitlement_immutable BEFORE UPDATE ON private_contact_entitlements
BEGIN SELECT RAISE(ABORT,'private contact entitlement is immutable'); END;
CREATE TRIGGER private_contact_entitlement_retained BEFORE DELETE ON private_contact_entitlements
BEGIN SELECT RAISE(ABORT,'private contact entitlement is retained'); END;

-- A second settled rail or excess ecash remains available for private reconciliation.
CREATE VIEW private_contact_excess_receipts AS
SELECT t.contact_id,'cashu' AS rail,r.acquisition_id AS receipt_id,
    r.net_sats - CASE WHEN e.rail='cashu' AND e.receipt_id=r.acquisition_id THEN 100 ELSE 0 END AS excess_sats
FROM private_contact_cashu_receipts r JOIN private_contact_tenders t USING(delivery_id)
JOIN private_contact_entitlements e USING(contact_id)
WHERE r.net_sats>100 OR e.rail!='cashu' OR e.receipt_id!=r.acquisition_id
UNION ALL
SELECT f.contact_id,'lightning',f.receive_id,f.amount_msat/1000
FROM private_contact_fees f JOIN private_contact_entitlements e USING(contact_id)
WHERE e.rail!='lightning';
