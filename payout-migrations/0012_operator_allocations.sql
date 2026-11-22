-- Planning evidence only. The payment worker must independently qualify custody
-- before reserving or submitting a transfer against one of these allocations.
CREATE TABLE payout_operator_allocations (
    sequence INTEGER PRIMARY KEY REFERENCES payout_intake_receipts(sequence),
    funding_hash TEXT NOT NULL CHECK(length(funding_hash)=64),
    collected_sats INTEGER NOT NULL CHECK(collected_sats>0),
    member_reserved_sats INTEGER NOT NULL CHECK(member_reserved_sats>=0),
    operator_sats INTEGER NOT NULL CHECK(operator_sats>0 AND operator_sats=collected_sats-member_reserved_sats)
) STRICT;
CREATE TRIGGER immutable_operator_allocation BEFORE UPDATE ON payout_operator_allocations
BEGIN SELECT RAISE(ABORT,'operator allocation is immutable'); END;
CREATE TRIGGER retained_operator_allocation BEFORE DELETE ON payout_operator_allocations
BEGIN SELECT RAISE(ABORT,'operator allocation is retained'); END;
