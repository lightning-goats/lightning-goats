# Private contact upgrade preparation

The live core still uses schema 23. Contact source adds schema 24 and remains
disabled. This preparation does not authorize or perform a migration, service
interruption, private-message delivery, payment, feeding or public publication.

Use only independently source-verified build artifacts and a separately
authenticated, root-owned tool closure. The eighteen-file closure now includes
`prepare-contact-upgrade.py`; do not invoke a writable checkout as root or reuse
an older seventeen-file manifest for the new tool. Independent archive SHA256s
must cover both the core release and CyberHerd review archive at the same full
source commit. The core archive supplies the daemon/CLI; the review archive
supplies the native four-journal tool. No Python from either archive executes.

Through the authenticated launcher, select `--contact-upgrade-prepare`, followed
by these six arguments in order:

1. Core release archive path.
2. CyberHerd review archive path.
3. Full common source commit.
4. Independently verified core archive SHA256.
5. Independently verified review archive SHA256.
6. New direct child of `/var/lib/lightning-goats-upgrades`.

Preparation requires the existing running core and overlay-only membership,
matching protected four-journal policy, original non-admin identities, zero
financial ceilings, absent membership signer and masked/stopped payout roles.
It freezes configuration, unit/drop-in, process and program identities; verifies
both archives; stages the three native programs and original file fingerprints;
and retains a private `PREPARED.json`. It opens no journal, reads no credential
and changes no installed program, configuration, unit or service. Reused paths,
changed baselines and incomplete output refuse. It provides no apply/resume
command. Do not execute its generic file transaction as a substitute for the
missing maintenance coordinator.

Before an actual upgrade, a separately qualified maintenance coordinator must
quiesce and fence all four writers, take and verify a **current** native schema-23
four-journal backup using the old installed tool, migrate separate copies as the
original runtime identity, and pass the new native `compare-contact-upgrade`.
The comparison preserves all original rows, row IDs, signed bytes, schema and
migration-history prefix while permitting only the exact additive DDL and empty
new tables. Repeat this comparison after actual migration and before resumption.
Keep outgoing roles masked and contact disabled until the optional configuration,
nginx ingress and browser flag receive their own verified activation.

Never roll an old binary back over a schema-24 database or restore stale paid
state. A behavior rollback must use a compatible new binary with the retained
original configuration and preserve all fee, payment, feeder and signed history.
External Strike/household reconciliation remains necessary for ambiguous work;
the comparison does not clear recovery markers or authorize resumption.

Synthetic native tests and the disposable installed systemd preparation fixture
are required. Neither qualifies an actual 100-sat fee or recipient-side receipt
of a private message. See the [contact checkpoint](../testing/website-contact-fee-checkpoint-20261111.md)
for exact accepted and pending source pins.
