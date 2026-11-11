# Private contact upgrade preparation

The live core still uses schema 23. Contact source adds schema 24 and remains
disabled. This preparation does not authorize or perform a migration, service
interruption, private-message delivery, payment, feeding or public publication.

Use only independently source-verified build artifacts and a separately
authenticated, root-owned tool closure. The source candidate now has nineteen
files, including `prepare-contact-upgrade.py` and `maintain-contact-upgrade.py`.
The already installed preparation-only generation has eighteen files and provides
no maintenance entry point. Do not invoke a writable checkout as root or reuse an
older manifest for the new closure. Independent archive SHA256s
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
still-unqualified maintenance coordinator.

## Maintenance executor candidate — not qualified or installed

The source candidate selects `--contact-upgrade-maintenance`. Its operations are:

- `plan REVIEW EXECUTION --review-sha256 INDEPENDENT_PREPARED_SHA256` freezes a
  separate three-program transaction without opening journals or changing services.
- `apply EXECUTION --exclusive-maintenance` fences all four writers, takes and
  verifies a current exact-schema-23 native backup, migrates a separate copy under
  the original UID in an isolated network namespace, and compares every retained
  row/schema/signature before program replacement. It then migrates the original
  core under the original UID, snapshots all four current journals and compares
  the actual result. All writers remain masked and stopped afterward.
- `resume EXECUTION --approve-resumption` independently rechecks the accepted
  images, comparisons, program/configuration identities and effective units. It
  restores the exact original core unit and membership mask state, starts only
  core plus membership, and verifies their process identities and health.

Contact configuration, nginx, static flags, boot enablement, membership signing
and financial limits do not change. The shared host maintenance lock excludes
these coordinators, while explicit administrative/database exclusivity remains
an operator responsibility. Changed/unknown artifacts, native refusal, unexpected
writers or interrupted execution retain evidence and stop further work. There is
no automatic reapply, old-binary rollback or database-restore command. After a
migration starts, even an ambiguous result must retain the new programs and all
journal bytes for inspection.

Pure tests and disposable protected-file fixtures exercise command admission,
private native receipts, mixed old/new program replacement states, backup and
clone rejection, changed financial state before restart and ambiguous live
migration. Native operations, kernel UID execution and systemd are mocked in
those Python fixtures. Exact root CI and a separate installed native/systemd
schema-23-to-24 rehearsal remain required before this candidate is used on a host.

The installed rehearsal uses only a guarded disposable CI VM, synthetic paid
state/credentials, an isolated network and a gateway mock that refuses POSTs.
Its harmless old-core ELF accepts the real future unit argv but opens no journal
or credential. A separate test-only operation removes only empty schema-24
contact definitions from the known synthetic fixture, after exact migration,
receipt and native signature checks. It cannot transform arbitrary state. The
root harness attests the isolated namespace in a protected marker; the helper
compares its own namespace without reading PID 1, which the hardened unit hides.
The native backup, clone/live migration, comparisons and resumed new services
must all actually pass. The stand-in is not evidence about an old production
daemon, and no fixture helper is included in production archives.

Before an actual upgrade, a separately qualified maintenance coordinator must
quiesce and fence all four writers, take and verify a **current** native schema-23
four-journal backup using the old installed tool or the separately qualified
`snapshot-before-contact`/`verify-contact-upgrade-source` native boundary, migrate separate copies as the
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
