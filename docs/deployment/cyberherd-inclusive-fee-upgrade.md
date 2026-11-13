# Inclusive-fee payout journal upgrade contract

This is the implementation contract for a required additive upgrade, not an
available production command or authorization to activate payments. The existing
same-schema program-maintenance transaction must continue rejecting this change.

The older payout prefix ends at migration version 8. Its compiled version list is
`1,2,3,4,5,6,8`: seven records, not eight. The new prefix is
`1,2,3,4,5,6,8,9,10,11`: ten records. Compare the exact ordered version/checksum/
success tuples against the compiled migrations; never infer migration history
from the highest version or accept arbitrary older prefixes.

The native source verifier must accept only this payout prefix with current core,
membership and signer schemas, protected policy, identical owner/epoch/provider
scope, native integrity and all existing cross-journal checks. Its source bundle
must remain ineligible for ordinary current-schema restore. Snapshotting retains
the existing all-four-writers-stopped/masked and unchanged-source file fences.

Compare a separate migrated copy before live application. Preserve every original
row ID, column, signed byte, table definition, index, trigger and migration-history
prefix in all four journals. Migrations 9–11 add exactly these payout tables:

- `payout_fee_observations`
- `payout_fee_probe_intents`
- `payout_fee_probe_callbacks`
- `payout_fee_probe_invoices`
- `payout_fee_probe_quotes`
- `payout_fee_promotions`
- `payout_fee_probe_duplicates`

All seven must be empty at the migration comparison boundary. New schema objects
must match the compiled SQL exactly. These migrations perform no old-row update,
backfill, invoice acquisition, signing, provider request or execution. Any such
change is a failed comparison. Preserve ambiguous/executed legacy quotes and their
original identities; a migration cannot reinterpret them as discovery probes.

Use a reviewed immutable artifact and a native backup accepted by both appropriate
source/current verifiers. Run migration under the existing payout runtime UID in
a network-isolated offline operation; do not start the payout worker to migrate.
Verify a fresh native four-journal copy after application. Keep all financial and
signing limits zero and outgoing roles masked throughout this upgrade. Selecting
the operator's inclusive-fee policy and enabling real execution remain separate
explicit activation steps after provider/recipient and recovery acceptance.

Required fixtures include a retained legacy quote/payment with unknown outcome,
unchanged signed requests and acknowledgments, the version-7 gap, dirty/unknown/
changed checksums, changed old rows/DDL, nonempty new tables, and actual distinct
runtime ownership. Failure after live migration must retain evidence and keep
writers held; do not restore stale paid snapshots or retry a fresh payment.

## Source-only backup entry points

`cyberherd-statectl snapshot-before-inclusive-fees --destination NEW_PRIVATE_PATH
--writers-stopped` and `verify-inclusive-fee-upgrade-source --bundle PRIVATE_BUNDLE`
are implemented in the development source. Both require protected root policy and
retain the existing four-owner file and native validation rules. The snapshot
selector changes only which exact compiled payout prefix is accepted; it does not
stop services, migrate journals, stage a restore, or enable an outgoing role.
Ordinary snapshots and restore still require the current schema. The snapshot, comparison and offline migration executor are implemented.
Production use still requires successful qualification of the complete installed
transaction against its exact immutable source, followed by operator review.

The development payout daemon now has an explicit
`--migrate-inclusive-fees --writers-stopped` child operation. It requires its own
non-root runtime UID, existing private journal, singleton lock, protected unchanged
disabled configuration, zero financial authority, exact old schema and matching
stored provider/intake bindings before applying the compiled migrations. It closes
SQLite on failure as well as success. It creates no grant/scope/intake binding and
never constructs the worker, loads the spending credential or opens a provider,
core or signer transport. Its local singleton lock is not a payment interface.
The stopped-writer flag is an attestation; the reviewed root coordinator must
independently check and hold all four services. Ordinary native tests exercise store migration under a non-root test UID. The
real service UID and complete installed transaction still require systemd
qualification before production use. Partial migration failure stays held for inspection, without an
automatic fresh migration or stale financial-state restore.

The six-program preparer now accepts an explicit `--inclusive-fee-upgrade`
selector for preparation only. It seals a different transaction kind while
retaining unchanged disabled configuration and no-activation/no-migration
preparation semantics. Ordinary apply, rollback and resume reject that review,
even with its correct digest. The preparation selector refuses other operations; apply and resume use the
dedicated schema command names described below. A digest must never
turn a schema-upgrade review into an implicit program-only deployment.

## Source coordinator commands (not production-qualified yet)

The authenticated tool now exposes separate `apply-inclusive-fees` and
`resume-inclusive-fees` commands with the reviewed preparation digest and their
explicit maintenance/resumption flags. Preparation uses `--inclusive-fee-upgrade
prepare`; application and resumption use their dedicated command names, without
that preparation selector. Program-only apply/rollback/resume continue rejecting
the schema-upgrade record.

The apply path accepts both native source verifiers, rehearses a payout copy under
its runtime UID, verifies all four journals, replaces the reviewed programs, then
records migration start before touching the live payout journal. A fresh native
snapshot and strict comparison must succeed before an applied receipt exists.
Resumption rechecks those exact snapshots, hashes, configuration and stopped
live images before restoring only core and membership. Outgoing roles stay masked.
No automatic database restore or old-program rollback follows migration failure.

Control-flow tests cover successful ordering, rehearsal failure before replacement,
and post-migration comparison failure with no service start. These mock native
parsers and systemd; actual-root files, real native migration and installed-systemd
qualification remain separate required evidence before any live invocation.

## Qualification status — November 13

Combined source `836b3db6dce32278ed8fea2b11b9251c35cb51cd` passed its full
[Rust and mandatory-root checks](https://github.com/lightning-goats/lightning-goats/actions/runs/43515662473)
and [dependency audit](https://github.com/lightning-goats/lightning-goats/actions/runs/43515662955).
Its package build, including the genuine pinned older verifier, passed. The
[installed-systemd run](https://github.com/lightning-goats/lightning-goats/actions/runs/43515663196)
failed before the CyberHerd scenario when systemd refused execution of the gateway
from the `/run` rehearsal directory. This is failed installed acceptance, not a
passing upgrade or permission to skip that gate.

Source `5df6fd581ca0cf8e9cdb9dc62cb2f84812d5d811` moves executable scratch
carriers to protected disk-backed storage and adds mount diagnostics. It preserves
all shipped sandbox directives. Its
[installed validation](https://github.com/lightning-goats/lightning-goats/actions/runs/43517864086)
passed package assembly and the general systemd sandbox but failed in the
inclusive-fee clone rehearsal: the coordinator applied the root-group-only
administrative-file guard to a correctly root-owned, payout-group-readable
configuration. A separate configuration guard now accepts only the installed
root:root `0644` layout or a root:payout-group `0440` copy, rejecting writable,
wrong-owner/group, linked or ACL-bearing files. Executable/admin-file guards are
unchanged. Actual-root regression cases and a fresh installed run must qualify
this correction. The separate nginx rollback fixture also needs its saved
executable-directory correction qualified.

No actual-host journal migration, spending-policy change or financial activation
follows from these source/disposable-host results.
