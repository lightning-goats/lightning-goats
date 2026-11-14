-- Presentation receipts only: no access to core or payout financial tables.
CREATE TABLE reward_result_cursors (
    owner TEXT NOT NULL,
    request_id TEXT NOT NULL,
    manifest_hash TEXT,
    total_rows INTEGER CHECK(total_rows BETWEEN 1 AND 1000),
    through INTEGER NOT NULL DEFAULT 0 CHECK(through BETWEEN 0 AND 1000),
    complete INTEGER NOT NULL DEFAULT 0 CHECK(complete IN (0,1)),
    poll_order INTEGER NOT NULL DEFAULT 0 CHECK(poll_order>=0),
    PRIMARY KEY(owner,request_id),
    FOREIGN KEY(owner,request_id) REFERENCES reward_plans(owner,request_id),
    CHECK((manifest_hash IS NULL)=(total_rows IS NULL)),
    CHECK(complete=0 OR through=total_rows)
);
CREATE TABLE reward_result_pages (
    owner TEXT NOT NULL,
    request_id TEXT NOT NULL,
    after_index INTEGER NOT NULL CHECK(after_index>=0),
    page_json TEXT NOT NULL,
    PRIMARY KEY(owner,request_id,after_index),
    FOREIGN KEY(owner,request_id) REFERENCES reward_result_cursors(owner,request_id)
);
CREATE TRIGGER immutable_reward_result_page BEFORE UPDATE ON reward_result_pages
BEGIN SELECT RAISE(ABORT,'verified result receipt is immutable'); END;
CREATE TRIGGER retained_reward_result_page BEFORE DELETE ON reward_result_pages
BEGIN SELECT RAISE(ABORT,'verified result receipt is retained'); END;
CREATE TRIGGER monotone_reward_result_cursor BEFORE UPDATE ON reward_result_cursors
WHEN NEW.owner IS NOT OLD.owner OR NEW.request_id IS NOT OLD.request_id
 OR (OLD.manifest_hash IS NOT NULL AND NEW.manifest_hash IS NOT OLD.manifest_hash)
 OR (OLD.total_rows IS NOT NULL AND NEW.total_rows IS NOT OLD.total_rows)
 OR NEW.through<OLD.through OR NEW.complete<OLD.complete OR NEW.poll_order<OLD.poll_order
BEGIN SELECT RAISE(ABORT,'verified result cursor cannot rewind or change identity'); END;
CREATE TRIGGER retained_reward_result_cursor BEFORE DELETE ON reward_result_cursors
BEGIN SELECT RAISE(ABORT,'verified result cursor is retained'); END;
