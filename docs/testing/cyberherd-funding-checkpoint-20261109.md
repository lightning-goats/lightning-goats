# Confirmed-feed funding verification

Code source: `b7ac0d50e77296eb77cf7dd20dbcaf474e077bfb`.
Scope: the [opt-in funding/reservation layer](../architecture/cyberherd-reward-funding.md),
private protocol version 2 and confirmation-atomic bookkeeping. The
[full CyberHerd port](../planning/cyberherd-implementation-plan.md) remains incomplete.
This source checkpoint does not install or activate a reward/payout service.

## Executed checks

| Check | Result |
| --- | --- |
| `cargo fmt --all --check` | passed |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | passed |
| `cargo test --locked --all-features` | 359 passed, zero failed, one ignored |
| Release/preflight/prepare/upgrade fixtures | 25 passed, two privileged fixtures skipped |
| `cargo tree -i rsa --locked --all-features` | no active reverse dependency |
| `cargo audit --ignore RUSTSEC-2023-0071` | existing unignored RUSTSEC-2026-0459 remains |
| `git diff --check` | passed |

The ignored Rust case remains the dedicated real-nak namespace qualification.
The release fixture invocation needed its sibling test directory on Python's
import path. A subsequent run hit two fixed two-second subprocess timeouts in
the non-regular-file rejection test during compilation. The unchanged full
27-case fixture set passed after compilation; neither its timeout nor assertions
were relaxed. The two skipped cases require privileged isolated host fixtures.
No host apply operation was executed by these checks.

The dependency graph/lockfile is unchanged. The refreshed 315-package audit with
1430 advisories still reports the documented SQLite issue. No new audit exception
or clean-security claim is introduced.

## Evidence from eight additional regressions

Synthetic settlements and virtual grants use actual temporary core databases:

- Tracking stays inactive by default. Bootstrap and repeated initialization
  preserve credit/event sequences, establish remaining FIFO lots and create no
  historical feeding funding record or retroactive reservation.
- Mixed credit consumes the actual oldest lots: a 1,000-sat feeding from 300
  virtual plus 1,000 real sats has 700 real sats consumed and a 70-sat ceiling at
  10%. Its next feeding can consume the remaining 300 real sats. Proof and exact
  reservation retries survive reopening without crediting or consuming twice.
- Operator grants, other providers/pools and payment-shaped uncorrelated ledger
  rows yield zero Strike reward funds. Real-funded proof reaches the private
  confirmed-feeding export; ordinary accounting is unchanged.
- A funding-write failure rolls back confirmation, debit, event and parts while
  retaining the original unresolved UUID/credit. Reconciliation with that UUID
  creates one proof/debit. Intent or unknown status cannot reserve a reward.
- Reservations serialize one owner/feed/plan budget, reject changed scope/amount/
  epoch, enforce zero limits and global outstanding limits, and do not change
  feed credit. Concurrent competing plans cannot both reserve the same UUID.
- An old unresolved reservation does not age out merely because two days passed.
  It still counts against the cap; no unknown payment budget is reused. Verified
  outgoing finality is still required for safe budget aging/release.
- Bootstrap failure leaves no partial activation. Changed receipt provenance or
  an untracked post-activation debit fails closed without manufacturing history.
  Source/cursor, consumed parts and reservation records cannot be reset through
  their ordinary update/delete paths.
- Actual private Unix framing/UID checks exercise the typed reservation method:
  missing root policy, shadow mode and wrong owner reject; an enabled exact plan
  returns one authenticated reservation and repeats the same ID. Changed plans
  fail, with no grant, provider request or physical command.

These sockets still use one test identity. They do not qualify provisioned
cross-UID roles, provider credentials/account binding, current Strike availability,
new signer kinds or actual food delivery. Correlated UUID retention is bookkeeping
evidence, not a physical test. No real provider request/payment was made.

## Remaining work

Funded provenance/reservations are not eligible payout plans or completed payouts.
Refresh current signed payee eligibility independently of historical interaction
profiles; freeze roster/points/shares, fees/minima and canonical plan hash before
reservation. The worker currently retains funding in its membership barrier but
does not create or submit those plans.

The outgoing service still needs its separate credential/account/environment
binding, bounded recipient/invoice/quote adapters, durable execute/reconciliation
state and authoritative terminal outcomes. The current global cap conservatively
counts every reservation as outstanding and cannot renew until that integration
exists. Only settled outcomes may produce treat notices. No implicit provider
balance, sweep change, member carry balance or fresh payment after ambiguity is
authorized by this checkpoint. Presentation, lifecycle, interfaces, recovery,
installation and explicit activation remain in the full contract.
