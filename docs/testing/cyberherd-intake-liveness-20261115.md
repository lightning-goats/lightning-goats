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

Qualification results will be recorded after the exact candidate gates finish.
Formatting, whitespace, published SQLite vendor verification and the active RSA
dependency exclusion check pass locally. The locked Rust regression build and
cached advisory audit are running. Fresh CI and installed qualification remain
pending. No generated build cache is a deployment artifact.
