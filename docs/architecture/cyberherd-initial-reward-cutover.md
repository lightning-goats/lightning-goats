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
has verified a complete core inventory. There is no installed production cutoff command yet. The source adapters below
implement these gates; combined installed acceptance must still prove them:

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

Native apply, append-only comparison, audited-exclusion reporting and the
financial binding are described below. Their combined installed acceptance is
still required. Historical production opportunities remain retained and unapplied.

## Native verification of recorded dispositions — development source

Ordinary four-journal verification now validates any retained cutoff audit against
the original core history through its recorded boundary. Later core events do not
move that boundary. Orphan/extra/missing audit rows, changed original proofs,
conflicting core reservations and nonterminal or malformed plans are refused.
Per-feeding input proofs remain checked even when the input has not reached a
membership barrier. Normal cross-journal checks still bind all payout/signing
records to their original grant and reservation; exclusions cannot hide them.

The reward reviewer validates the same audit before classifying a barrier as
`excluded_before_activation`. It retains the original total pending-feeding count
and separately reports `pending_reward_eligible_feeding_ingress`. A scoped cutoff
summary records the decision ID, epoch, boundary and source-backup hash. This is
not activation permission. The financial adapter accepts the new excluded stage only through the explicitly
verified completed-cutoff binding described below.

New native fixtures cover excluded frozen and deferred inputs, future feedings
remaining unplanned/eligible, unchanged original barriers, and tampered/missing
audit or conflicting plans. The earlier preview source passed full Rust and
installed Deployment qualification; these additional verifier/report changes still
require their own exact-source CI. The native apply/comparison and coordinator source are described below; combined
installed qualification remains open. No live disposition has been applied.

## Offline membership child — development source

`cyberherdd --record-initial-reward-cutover PROTECTED_PREVIEW_FILE
--reward-cutover-sha256 REVIEWED_SHA256 --writers-stopped` is a dedicated offline
source entry point, mutually exclusive with configuration checking and fresh
initialization. It never starts runtime workers or opens sockets, credentials or
other journals. It requires unchanged protected configuration with reward planning
absent, its separate non-root member UID, the same state-scoped kernel singleton
as the runtime, an existing private database with exact current migrations, and
a private root-protected digest-pinned preview matching owner/epoch/binding.
Recovery markers, unprotected input, missing files and schema mismatches refuse
without initialization or migration. The bounded output is the immutable domain
receipt and grants no activation authority.

The stopped-writer switch is a parent attestation, not independent proof that
other units are stopped. A qualified root transaction must still prove those
unit guards, take a fresh verified four-journal checkpoint, derive/pin the preview,
rehearse under the member UID, compare all retained rows, apply under the same
fences and verify the final checkpoint. The child alone must never be treated as
a production approval or four-journal recovery tool. No installed launcher invokes
it yet. New tests exercise CLI argument fencing, private/current existing-only
journal opening, recovery refusal and unprotected-preview rejection; actual-root
positive child and installed transaction qualification remain required.

The required actual-root CI harness now includes a disposable root/member-UID
round trip: fresh synthetic current-schema four-journal state, native backup and
preview, root-protected group-readable preview, member-owned existing journal,
separate non-admin child process, exact retry and wrong-pin/attestation refusals,
then the native append-only comparison. The child executable is disk-backed;
network remains isolated by the harness. Root execution is refused and a changed
mock writer guard refuses a new live checkpoint; isolated accepted image comparison remains read-only. This tests filesystem/UID and native
comparison authority, not real systemd masking or production deployment. It must
produce its explicit positive marker in exact-source CI before being qualified.

## Reviewed coordinator — development source

The authenticated root tool closure now includes
`maintain-cyberherd-reward-cutover.py`, selected with
`--cyberherd-reward-cutover-maintenance`. It consumes an existing financial
transaction's fresh stopped-writer checkpoint; it does not independently stop
services or replace programs, configuration or credentials. The installed native
programs must already contain the qualified cutoff commands.

All operations retain the financial preparation, checkpoint and original reward
report SHA256 pins. `prepare --decision-id UUID` verifies that original checkpoint,
derives the native preview, runs the member child on an isolated copied journal,
and proves the exact four-journal append-only comparison. Its private
`reward-cutover/PREPARED.json` pins the preview, expected receipt and rehearsal.
The live journals must remain unchanged. Incomplete preparations cannot apply.

`apply --cutoff-review-sha256 SHA --approve-initial-reward-cutover` separately
requires explicit maintenance authorization. It rechecks the original stopped
images and retained rehearsal, records the start before running the member child,
and requires the exact same receipt. It then takes/verifies a new four-journal
backup, compares it to the original with the reviewed preview, and obtains a new
native reward report whose cutoff summary matches the decision. Only successful
verification produces `reward-cutover/APPLIED.json`. Configuration, payout/signing
workers and existing ledger rows remain unchanged. Any failure retains the stopped
state and start receipt; there is no automatic replay, rollback or database restore.

`resume-original --cutoff-review-sha256 SHA --applied-sha256 SHA
--approve-original-resumption` verifies the completed disposition and unchanged
post-cutoff live images before restoring the original core/member units. This
resumes normal receive/feeding behavior under the existing configuration and
trusted gateway controls. Outgoing roles remain masked. Resumption is one-shot;
a partial start requires inspection rather than automatic retry.

The original financial checkpoint still records pre-cutoff bytes. Do not reuse it
for financial configuration application or restore its old journals after the
cutoff. A subsequent financial plan/checkpoint and explicit validated-cutoff
binding remain required. Without the explicit verified-cutoff binding below, the financial backlog gate
continues to refuse audited exclusions.
No cutoff application, service resumption or financial activation has occurred on
the production host through this new source.

Focused coordinator tests cover protected files with mocked native/service
operations, including no approval, changed original images, comparison refusal,
post-apply failure, absent/unreviewed completion, and original-only resumption.
The mandatory Rust fixture separately exercises actual root/member UIDs, native
journal application and comparison. Launcher routing, exact tool inventory/export
and existing financial activation regressions also pass locally. These are source
and disposable-fixture results, not an installed combined systemd acceptance or a
production approval.

## Exact cutoff binding for financial planning — development source

A new financial plan may take `--initial-reward-cutover CUTOVER_DIRECTORY`,
`--cutoff-preparation-sha256 SHA` and `--cutoff-applied-sha256 SHA`. All three are
required together. The directory is the earlier transaction's private
`reward-cutover` directory; it is data, never an executable tool source.

Planning and every subsequent load verify the completed preparation/application,
original checkpoint and scope, original-only resumption, and retained before/after
images. They execute only the current fingerprint-pinned installed native verifier:
verify the post-cutoff bundle, repeat the append-only comparison, then recompute a
fresh reward report from that accepted bundle. Saved summaries alone are not
authority. The newly sealed financial preparation pins the full reviewed binding.

The native report summary now includes `decision_sha256`, SHA256 of the complete
typed decision serialized in native struct field order, including every historical
UUID/proof and the decision time. A fresh financial report must retain this exact
hash, ID, epoch, boundary, source-backup hash and inventory count. Matching IDs or
counts do not substitute for the complete decision. Native comparison also returns
the decision hash; preparation refuses an older acknowledgement before any live
audit is written. This changes private report/acknowledgement shape, not a journal
schema or the stored decision. Rust/Python fixed-vector tests pin the serialization.

With that verified binding, native `excluded_before_activation` barriers and
historical pending feeding inputs remain retained without rewards. Only later
confirmed feedings may remain as unplanned funded opportunities. The audited
cutoff is their eligibility boundary; enabling a worker later does not move it.
Unreceived financial events, unclassified eligible pending feedings, existing
reservations/awaiting plans, held money, unknown stages, changed settings, missing
proof or a different decision still refuse first activation. Without a binding,
the previous strict no-funded-backlog path remains and any cutoff summary refuses.

Protected-file tests cover full binding revalidation, fresh native-summary drift,
plan/load propagation, future-only gate behavior and configuration application
without starting workers. Root coordinator and financial tests remain synthetic
with native/service calls mocked; the mandatory Rust UID/store fixtures qualify
actual audit persistence separately. Exact-source Rust/Clippy, security and
installed acceptance of this new hash/binding are still required. No production
cutoff or financial activation is performed by these source changes.

Local qualification for the current binding includes 11 coordinator tests,
17 financial-activation tests and the complete 182-test protected root upgrade
suite in an isolated network namespace. The grouped fixture explicitly redirects
the shared maintenance-lock helper into its disposable root; no production
maintenance lock is needed. Rust changes are formatted locally and await the
required exact-source CI build/tests rather than compiling on the production VPS.

## Combined installed rehearsal — development source

The disposable systemd rehearsal now exercises the cutoff coordinator and its
subsequent financial binding with actual installed programs, journals and UIDs.
It first verifies the retained accepted bundle and refuses preview specifically
because the fixture's original feeding is still ambiguous. After the earlier
upgrade-preservation proofs finish, the guarded CI operator stops the writers
and reconciles only that known synthetic UUID as fed through the native offline
CLI. This is synthetic accounting, not a gateway command or real food delivery.
The signed payment history and UUID remain retained; the explicit debit becomes
the baseline for later preservation assertions.

A read-only test-only member probe waits for that confirmation's frozen barrier.
The actual coordinator then prepares/rehearses/applies/verifies the cutoff and
resumes only the original services. The report must classify the single historical
feeding as excluded and retain its full decision hash. A new candidate and new
financial plan capture the resumed process identities and bind the independently
reviewed cutoff receipts. They take a new checkpoint before configuration and
four-role activation on the disposable VM. Synthetic legacy funding is unavailable
for payouts; signer calls, provider operations and gateway POSTs remain zero.

The resulting rehearsal evidence distinguishes this explicit synthetic resolution
from the earlier proofs that preserved an unresolved UUID across upgrades. It does
not claim physical or real-provider qualification. Never perform a reconciliation
against production merely to make a cutoff pass.

The preceding deployment run at source `d5284c1` passed package/unit qualification
but failed in installed four-role candidate validation, before the new cutoff
sequence. The checker now emits only a static rejection category; arbitrary parser
and policy details remain private. The guarded CI exception path may re-probe only
its fingerprint-matched synthetic checker/config carriers to retain that category
or an execution error. The production command wrapper still withholds subprocess
output. A fresh successful combined installed run is required; these source
changes do not establish it or authorize live financial activation.
