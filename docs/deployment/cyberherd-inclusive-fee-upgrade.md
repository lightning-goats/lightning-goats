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
