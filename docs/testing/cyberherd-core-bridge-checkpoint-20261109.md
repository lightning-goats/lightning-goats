# Private core-event bridge verification

Code source: `496c4d15486e243e434e965e8b41315d50e603e4`.
Scope: the [optional private bridge](../architecture/cyberherd-core-bridge.md),
its source bindings and actual daemon/config wiring. This is not completion of
the [full CyberHerd port](../planning/cyberherd-implementation-plan.md).

## Executed checks

| Check | Result |
| --- | --- |
| `cargo fmt --all --check` | passed |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | passed |
| `cargo test --locked --all-features` | 338 passed, zero failed, one ignored |
| Release/preflight/prepare/upgrade Python fixtures | 25 passed, two skipped |
| `cargo tree -i rsa --locked --all-features` | no active reverse dependency |
| `cargo audit --ignore RUSTSEC-2023-0071` | existing unignored RUSTSEC-2026-0459 remains |
| `git diff --check` | passed |

The one ignored Rust case is the existing dedicated real-nak namespace test.
The two skipped Python cases require the privileged isolated installation/probe
fixture; they were not executed by this non-admin check. The first Python run
hit the workspace sandbox's read-only `/var/tmp` in three temporary-file cases.
The unchanged fixtures passed when rerun with their ordinary temporary-directory
access; no host apply operation was run. The dependency graph is unchanged and
the fresh audit still reports only the documented SQLite advisory. No new ignore
or clean-security claim is introduced.

## What the tests prove

- Genuine settlement/credit provenance exports payment authority, including
  canonical payment hashes. Copying payment/feed event names and payloads into
  other log entries does not create that authority.
- New confirmed-feed events bind their UUID, debit, status, completion and event
  atomically. Injecting a binding-write failure rolls back confirmation/debit;
  retry uses the original UUID. Bindings cannot be updated or deleted.
- Native settled-zap export retains exact raw request bytes and authoritative
  paid time without requiring signing/publication. Mismatched P2P amounts remain
  actual credit without qualified zap authority.
- Count/byte-bounded pages retain contiguous positions. A changed epoch, ahead
  cursor, history gap or corrupt source fails validation rather than resetting
  or guessing. Large native context resumes in another complete page.
- Actual private Unix sockets exercise kernel peer checks at both ends, wrong
  peer rejection, oversized frames and unknown mutation/SQL-shaped requests.
  No such request changes financial state.
- Binding refuses live sockets, regular files, symlinks and writable peer
  directories. Only an unchanged stale owned socket is reclaimed. Admission has
  a finite four-request burst and integer monotonic refill before frame parsing.
- Safety expires to unknown; shadow mode never reports readiness. The optional
  bridge is outside the daemon's critical worker exit selection.
- Existing settlement, feeder/gateway, native-zap, overlay, outbox, migration and
  operator-credit regressions passed with the new additive schema.

The positive socket fixture runs under one non-admin test identity. It does not
prove provisioned cross-UID access, group/credential containment or a running
CyberHerd consumer. No new signer-kind, public relay, real payout or physical
feeding qualification is claimed. Consumer sequencing/checkpointing, observation
refresh, real-fund reward planning, isolated payouts, note/chat delivery,
interfaces, units and installation/activation remain in the full plan.
