# Initial future-only reward cutover

The operator selected future confirmed feedings only on 2026-11-14. The
[actual staged candidate and historical review](../testing/cyberherd-financial-candidate-live-staging-20261114.md)
record the input: historical positive allocations and additional deferred feeding
inputs must not become retrospective payouts when rewards are first enabled.

## Domain persistence and enforcement

The source domain stores one immutable-in-application decision per project in
`operator_commands`, using the reserved non-UUID key `initial-reward-cutover`.
Each historical feeding gets a separate indexed audit entry keyed by its UUID.
These keys are outside the signed HTTP operator command's UUID namespace. There
is no new table, schema migration, HTTP command or payout-bridge capability.
The decision retains its ID, core epoch/event boundary, backup manifest hash,
operator decision time and ordered bounded feeding inventory/proof hashes.
The receipt always grants no activation authority.

The trusted offline caller must supply the complete inventory from a fresh,
quiesced, accepted core backup. Domain recording rejects wrong/stale epochs,
missing or changed already-received feeding proofs, replaced decisions and
existing nonterminal reward authority. It commits the decision and all exclusions
atomically. Exact retry returns the same receipt and verifies retained exclusions.
Existing snapshots, ingress, paid state, roster, feed credit, plans and messages
are not rewritten. Previously closed no-payout plans remain retained.

Reward planning and export skip excluded UUIDs; direct attempts to freeze a new
plan for an excluded feeding fail. Intake checks historical events against the
inventory before advancing its cursor. Missing historical proofs or changed
excluded proofs fail the whole intake transaction. A historical feeding received
later remains excluded, while a genuinely later confirmed feeding follows normal
planning and funding rules. Exclusions grant no payout, reservation or physical
feeding capability.

## Qualification and unfinished operator boundary

The initial source adds native temporary-store regressions for frozen plus
not-yet-received feedings, later eligibility, preserved historical snapshots,
restart/retry, conflicting plans, omitted/changed proofs and cursor rollback on
an incomplete delayed inventory. Formatting passes locally. Full locked Rust,
Clippy, security and installed qualification still need exact-source CI.

The store API is a trusted internal domain boundary, not a proof that its caller
has verified a complete core inventory. There is deliberately no executable
production cutoff command yet. The native operator adapter must still:

1. Pin a fresh four-journal checkpoint, core epoch and event head; derive every
   historical confirmed UUID/proof without treating deferred inputs as future.
2. Reconcile original confirmations/funding and reject any conflicting core,
   payout, signer or reservation authority before disposition.
3. Preview and seal the exact inventory/decision for review, then apply only
   its append-only audit entries under stopped-writer and ownership fences.
4. Verify original paid/signed/frozen rows and a fresh post-disposition snapshot.
   The native reward report must classify audited exclusions and distinguish
   excluded pending feeding inputs from still-eligible pending financial work.
5. Bind the completed cutoff and fresh review into first financial activation.
   Missing or inconsistent evidence must keep activation disabled.

The source domain implementation does not satisfy these operator steps or the
full recovery-adoption obligation. No live cutoff, historical payout, reservation
release, feeder command or financial activation has been performed by this change.

## Native backup-derived preview — development source

`cyberherd-statectl preview-initial-reward-cutover --bundle PRIVATE_ACCEPTED_BUNDLE
--destination NEW_PRIVATE_PREVIEW --decision-id UUID` now derives the inventory
from the accepted core event head and protected membership binding. This is source
functionality, not an installed production command or cutoff application.

It reuses the authenticated bridge's core projection to validate original confirmed
UUIDs, matching debits, consumed funding and feeding presentation. It checks the
retained event interval for gaps, scans confirmed feeds exhaustively in bounded
pages and validates the same typed event envelope. A truncated or contradictory
source refuses; no supplied historical UUID list substitutes for that scan.
Already-received membership proofs must match the independently derived inventory.

Preview refuses pending physical work, existing reservation/grant/signing/execution
history, held or unresolved provider money, nonterminal plans and an earlier cutoff.
It reads only verified backup images and rechecks source hashes/native reports before
writing a private preview. Output retains backup identity/hash, owner/policy scope
and the complete bounded decision. It applies nothing and grants no activation.

New native tests compare the read-only backup projection with the authenticated
bridge projection, include a not-yet-received feeding, and reject wrong epoch and
stale head. The mandatory root fixture additionally refuses preview of its retained
pending physical work before creating output. Local formatting and focused Python
checks pass; the new native preview still requires exact-source CI qualification.
The earlier cutoff-domain source passed full Rust CI but its installed scenario
failed before cutoff work when the runner refused a fixture executable under `/run`.
The rehearsal now keeps executable copies in protected disk-backed storage while
retaining `/run` database and attestation paths. Shipped sandbox policies are unchanged.

The apply adapter, append-only post-cutover comparison, audited-exclusion native
classification and financial-activation binding remain unfinished. Historical
production opportunities are still retained and unapplied.
