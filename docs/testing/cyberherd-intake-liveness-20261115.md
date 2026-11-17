# Optional social intake liveness — 2026-11-15

Status: source correction under qualification; not installed. The hotfix starts
from production source `4230636c4286dab3ddc1e993d18544d893b31870` and contains
no Cashu changes, schema migrations or dependency changes.

## Observed incident

Read-only live ledger inspection confirms the 1,000-sat Zap Cooking payment at
18:50:15 UTC and feeder confirmation at 18:50:20 UTC, UUID
`bd40f6d9-39e8-4603-87d0-5e11fa3955b8`. CyberHerd had received that payment and
confirmation, but its oldest pending social repost had no available author
profile. The worker returned `Waiting` indefinitely, preventing those subsequent
inputs from producing a membership snapshot and reward plan. The actual blocking
repost had a nonempty embedded body; empty kind-6 content is independently valid
when references authenticate its target.

## Correction

- Park optional social work when its profile or eligibility is unavailable.
  Atomically retain an explicit disposition on the ordered entry and retain the
  signed event, inbox state and existing retry backoff.
- Retry one due parked event only when ordered pending work has drained, using a
  new queue position and the original domain source identity. Late eligibility
  cannot alter a prior feeding snapshot or create a second interaction award.
- Ignore malformed/unsupported legacy social entries with an audit disposition
  before runtime target/profile lookup can fail on them repeatedly.
- Restrict this completion path to the linked social head. Payment/feed entries,
  real storage failures and inconsistent authenticated state still fail closed.
  Pause observations still restrict all queued paid work before social screening.
- Manual cycle resets close pending parked inbox work in the same transaction,
  including same-second profileless events, without deleting their signed input.

No live row/cursor edits, manual reward plans, replacement payout intents,
provider transfers, physical actions, service replacements or limit changes are
part of this source qualification.

## Regression evidence

The ordered-ingress tests exercise actual separate temporary core/membership
SQLite databases and authenticated Unix-socket fixtures:

1. Empty repost plus missing profile, followed by a paid zap and confirmed
   feeding: financial work drains across a store reopen; late profile recovery
   cannot alter the original snapshot, duplicate credit or reopen a completed
   social identity.
2. Unknown social eligibility: financial work drains, retries preserve the
   original frozen profile, retry positions advance and exponential backoff
   survives reopen.
3. Malformed/unsupported legacy social heads: runtime screening writes an ignored
   result and allows later paid/feeding work to proceed.
4. A late transaction failure rolls back both checkpoints; a social disposition
   cannot resolve a financial head.
5. Manual reset rollback preserves parked work; successful reset prevents it
   from earning a new-cycle award while retaining replay receipts and raw input.

The first integrated candidate `b5ed0d883e612555b83bf28520daec865c5d9a2c`
passes [formatting, strict all-target/all-feature Clippy, the locked full Rust
suite and root-owned state checks](https://github.com/lightning-goats/lightning-goats/actions/runs/43773027041).
The library suite reports **456 passed, zero failures, six explicitly ignored**;
all five cases above pass, alongside the integration suites. The
[fresh advisory audit](https://github.com/lightning-goats/lightning-goats/actions/runs/43773027095)
and [isolated signer](https://github.com/lightning-goats/lightning-goats/actions/runs/43773027043)
also pass. Its release/systemd qualification is still running at this checkpoint.

A subsequent retry-query refinement selects the original `nostr:<event-ID>`
through the existing unique owner/source index, avoiding scans of completed
history for every parked event. Exact final-source CI must qualify that refinement
before release selection. Formatting, whitespace and published SQLite vendor
verification pass locally; RSA remains absent from the active dependency graph.
A separate cached advisory audit passes against 1,509 local advisories. Native
host compilation remains in progress. Installed qualification is pending, and no
generated build cache is a deployment artifact.
