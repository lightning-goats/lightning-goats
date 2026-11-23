-- No row means Lightning to the member's lud16. Keep the revision after revocation
-- so a stale signed request cannot silently restore an older destination.
CREATE TABLE member_reward_preferences (
    owner TEXT NOT NULL REFERENCES owners(owner),
    member TEXT NOT NULL CHECK(length(member) = 64),
    revision INTEGER NOT NULL CHECK(revision > 0),
    method TEXT NOT NULL CHECK(method IN ('lightning','public_nutzap','private_ecash')),
    payment_request TEXT,
    CHECK ((method = 'private_ecash') = (payment_request IS NOT NULL)),
    CHECK (payment_request IS NULL OR length(payment_request) BETWEEN 1 AND 8192),
    PRIMARY KEY (owner,member)
);
