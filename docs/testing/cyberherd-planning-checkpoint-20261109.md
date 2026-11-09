# Eligible reward planning — verification in progress

Code source: `fb0ee590c8d0cc3924b6368068382012ff1434e8`.
Scope: [immutable eligible reward planning](../architecture/cyberherd-reward-planning.md),
the advisory core allowance, bounded planning rounds and consistent public-address
validation. This is not full-port completion, installation or payout activation.

## Current evidence

| Check | Observed result |
| --- | --- |
| Formatting on the code source | passed |
| Final strict locked all-target/all-feature Clippy | running; earlier planning source passed |
| Final locked all-feature Rust suite | pending final Clippy |
| Separate worker build and non-applying CLI | build passed; three CLI tests passed |
| Release/preflight/prepare/upgrade fixtures | 25 passed, two privileged fixtures skipped |
| Refreshed dependency audit | open unignored RUSTSEC-2026-0459 remains |
| RSA reverse dependency | unreachable from active graph |
| Dependency/lockfile changes | none |

This checkpoint deliberately retains pending status until the final check handles
finish. Do not cite the earlier funding suite's 359 passing tests as evidence for
this newer source. The ordinary real-nak qualification remains a separate fixture.

The first planning run passed ten of eleven reward tests. Its expected mixed-fund
split incorrectly assumed full-rational allocation after the required six-decimal
share step. At a 70-sat pool the frozen 7.142857%, 2.142857% and 0.714286% shares
yield gross amounts 49, 14 and 5 sats. Two sats remain unallocated; with one-sat
fee allowances the reserved gross is 68 and remaining confirmed credit is 932.
The corrected expectation preserves that required rounding and no-carry behavior.

An intermediate full library run then passed 234 tests and failed one existing
signer deadline fixture: its 300ms child process had not created its PID file.
All eleven reward tests, two recipient-metadata tests and the private allowance
test passed in that intermediate source. The failure occurred during concurrent
worker compilation; resource contention is a possible cause, not a proven fix.
No signer assertion or timeout was changed. The final source additionally rejects
reserved domains, numeric IP aliases and dot path segments consistently before
membership/planning; its final full run is still required.

The first deployment-fixture invocation failed because the filesystem sandbox
made its temporary `/var/tmp` directory read-only. The unchanged permitted rerun
passed all 25 non-privileged fixtures. The two privileged skips remain unexecuted,
not passing host-install evidence. No host apply command was used.

## What the regressions exercise

- Confirmed-feed mixed real/virtual funds, exact six-decimal/whole-sat rounding,
  fees and unallocated amounts; exact reservation attachment across reopening.
- New signed negative profiles, unavailable/stale/future reads and head changes
  during resolution cannot restore an older interaction's address.
- Receiver minimums, lookup failure, duplicate addresses/callbacks and operational
  destinations skip without carrying a member balance or minting another budget.
- Zero allocations/virtual-only feedings close without lookup/backpay. Capacity
  limits reject an over-budget freeze without leaving a partial plan.
- Immutable retained owner-scoped plans, authenticated core UID/epoch/funding/hash
  bindings and reservation retries before checking newly available capacity.
- Exact raw metadata hashes, bounded optional thumbnails/non-string metadata,
  secure callback syntax, explicit unsupported fallback and rejected malformed
  advertised Nostr support. Recipient discovery requests no invoice or payment.
- A bounded worker round uses staged exact-head metadata to freeze a plan; after
  a lost reservation response another round attaches the same core reservation.
  Private framing exercises actual kernel credentials with one synthetic test
  identity, not the provisioned separate-UID deployment.

## Remaining qualification and implementation

The [outgoing contract](../architecture/cyberherd-outgoing-contract.md) records
verified Strike endpoints and required ambiguity-safe recovery. It does not
implement the payout identity, account/credential binding, signing broker, invoice
callback, quotes/execute/reconciliation or authoritative terminal budgets.
Reservations still conservatively remain outstanding indefinitely.

Treat messages need verified completed payouts and real joining-message receipts;
private plans cannot be broadcast wholesale. Full conversation delivery, lifecycle,
interfaces, recovery/installation and explicitly selected financial activation
remain in the [twelve-part plan](../planning/cyberherd-implementation-plan.md).
No production test credit, provider payment, public post or physical action was
created by this implementation/checkpoint.
