# CyberHerd ordered worker verification

Code source: `c8b53e1eb7951474e013536c115d7ca37e3e5387`.
Scope: the [separate intake worker](../architecture/cyberherd-worker.md),
ordered receiving, settled-zap membership and feeding membership barriers.
The [complete twelve-part implementation contract](../planning/cyberherd-implementation-plan.md)
remains incomplete. This checkpoint installs or activates no CyberHerd service.

The upstream explanation was rechecked through its current file history and raw
contents. Its latest file-changing commit remains
`92ecf6c52482de17d404dbf2def3c71213408140`; the plan's reference is unchanged.

## Executed checks

| Check | Result |
| --- | --- |
| `cargo fmt --all --check` | passed |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | passed on final source |
| `cargo test --locked --all-features` | 351 passed, zero failed, one ignored |
| Explicitly selected built worker CLI fixtures | two passed |
| Temporary systemd definition verification | passed with nonexecuted harmless executable path |
| `cargo tree -i rsa --locked --all-features` | no active reverse dependency |
| `cargo audit --ignore RUSTSEC-2023-0071` | existing unignored RUSTSEC-2026-0459 remains |
| `git diff --check` | passed |

The ignored Rust case is the existing real-nak network-namespace qualification.
The original socket test run encountered sandbox-denied peer-credential access;
the tests passed with ordinary private Unix socket access. The systemd verifier
likewise required ordinary local socket access. Neither operation installed a
unit or invoked a host apply helper. The unit check substituted `/usr/bin/true`
only as an existing executable path; it did not execute it and does not verify
an installed CyberHerd binary, runtime identities, network filters or DNS.

The dependency graph/lockfile is unchanged. The fresh audit scanned 315 packages
against 1430 advisories and still reports the documented SQLite vulnerability.
It is neither ignored nor claimed fixed. Private store permissions do not turn
that dependency gate green.

## Receiving and ordering evidence

Thirteen additional Rust regressions cover the worker/receiver and runtime
boundaries. Synthetic settled payments run through actual temporary core and
membership databases, native signed requests/invoices, core proof export and
private client framing/UID checks. They establish:

- Two distinct paid invoices reusing the exact signed request both count, using
  authoritative paid time rather than the payer's older request timestamp.
- Missing profile/classification holds a feeding's membership barrier behind preceding payments. A
  later staged, backdated social event cannot enter the earlier feed snapshot.
- A failed receiving checkpoint rolls back queued input, source receipts and
  cursor together. Replayed changed envelopes, epochs or financial facts fail;
  a changed local settlement row ID does not create another domain interaction.
- A crash after membership commit but before queue completion reuses the frozen
  receipt/profile and leaves scores unchanged even during discovery failure.
- A feeding snapshot and queue completion are atomic. Injected snapshot failure
  leaves the feeding pending; the eventual owner/UUID snapshot remains unchanged
  after later members join.
- Observed pause restricts all already queued payments, including those behind
  the head. Missing profiles and alternate retry paths cannot restore broader
  project-note eligibility. Closed-cycle inputs discard without profile lookup;
  current-cycle unavailable discovery remains deferred.
- Full intake rolls back upstream progress and raw event insertion, and the
  runtime distinguishes typed backpressure from identity/storage failures so it
  can drain existing work. Virtual credit/unlinked events create no zap work.
- Discovery includes host `p` queries, persists signed negative profiles and
  withholds availability when the configured relay quorum is missing. Saturated
  social backfill cannot advance its durable window; duplicate raw observations
  reuse one interaction.
- The Linux state lock rejects a second binding, scopes by state/UID and
  releases without stale files. Only typed capacity pressure allows the worker
  to continue draining after an intake error.

The two CLI checks use an explicitly selected built `cyberherdd`: valid `--check`
and disabled normal startup leave state/socket paths uncreated. Invalid core
identity, an excessive relay quorum and an attempted credential field fail
validation. These are non-applying checks, not a positive root-configured,
cross-UID runtime installation test.

## Limits and remaining scope

The private transport fixture uses one test identity plus kernel UID comparison;
provisioned cross-UID group access and containment still require qualification.
Mock relay completion does not prove completeness of public relay history.
Saturated single-second backfill holds for recovery rather than losing events;
further pagination/operator recovery and retention/capacity operations remain.

Feeding barriers freeze membership, not payout eligibility or money. They provide
no real-fund reservation, fee budget, provider availability, outgoing payment or
public treat evidence. Current eligible payees must be refreshed separately from
historical interaction profiles before planning. Real-fund accounting/reservation,
isolated payout/reconciliation, note/chat/overlay outboxes, lifecycle notices,
public roster/OBS, authenticated operator/client tools and installation/activation
remain in the full plan. No new signer-kind, real payout or physical feeding
qualification is claimed.
