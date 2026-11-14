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
