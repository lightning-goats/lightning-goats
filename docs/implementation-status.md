# Phase 1 Implementation Status

This file tracks the current implementation state while the existing LNbits production stack remains authoritative.

## Canonical `clnaddress` dependency

The canonical Lightning Goats fork is now:

```text
lightning-goats/clnaddress
```

The Phase 1 v1 contract was squash-merged to `master` at:

```text
33117e85d39d137161dd3e4342c5296f2c1da911
```

That commit defines the server-generated invoice-label contract consumed by `lightning-goatsd`:

```text
clnaddress:v1:<user>:<uuid>
```

and includes strict usernames, per-address min/max policy, LUD-12 comment validation, per-address Nostr policy, protected file-based Zap signer loading, atomic registry/cursor persistence, privacy hardening, and integration tests.

The fork passed its full inherited PR matrix across CLN 25.09.3, 25.12.1, 26.04, and 26.06.6, including Rust 1.85 MSRV verification, Rust build/unit tests, and integration pytest on all eight matrix jobs.

## Implemented in `lightning-goatsd`

- Rust 1.88 crate with `#![forbid(unsafe_code)]`.
- Strict `clnaddress:v1:<user>:<uuid>` invoice classifier.
- Durable SQLite database with WAL, `synchronous=FULL`, foreign keys, and migrations.
- Explicit one-time CLN `pay_index` cursor initialization.
- Atomic paid-invoice recording, herd crediting, event creation, and cursor advancement.
- Duplicate and out-of-order settlement protection.
- Durable feed-credit ledger.
- Serialized multi-threshold feed accounting.
- Persistent feed intents and confirmed feed debits.
- Restart reconciliation from interrupted intent to `unknown` without debit.
- Local-only ambiguous feed reconciliation (`fed` / `not-fed`) with no feeder actuation path.
- Restricted CLNRest `waitanyinvoice` adapter using the systemd rune credential.
- CLNRest loopback-only enforcement and verified TLS with optional local CA.
- OpenHAB override reader and automatic feeder adapter.
- Shadow mode that never actuates the feeder.
- Active feed worker that checks override before every feed.
- Ambiguous OpenHAB outcome fails to `unknown` and never automatically retries.
- Durable overlay event log.
- Payment/feed/error events committed atomically with the state changes they describe.
- Race-free overlay snapshot state and event sequence.
- Read-only `/ws/overlay` durable replay stream.
- Read-only `/healthz` and `/api/v1/status` endpoints.
- Local operator CLI for cursor initialization/status/feed reconciliation.
- `nak` NIP-46 signing adapter.
- Shadow-safe durable Nostr message cursor.
- Transactional signed-event outbox.
- Exact signed Nostr event retry rather than re-signing/re-IDing failed publications.
- Active-mode-only access to the NIP-46 client credential.
- Hardened `lightning-goatsd` systemd unit.
- Hardened `nak` NIP-46 bunker systemd unit and wrapper.
- nginx canary configuration example.
- server setup runbook.

## Verification

The integrated Rust runtime, including durable overlay and Nostr/outbox workers, has been verified with:

```text
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

The obsolete local `clnaddress` patch artifact has been removed now that the canonical org fork is merged.

## Next

1. Add idempotent legacy LNbits opening-credit import.
2. Add idempotent late-LNbits settlement import and audit records.
3. Build the temporary legacy reconciliation command/timer used only during cutover grace.
4. Port/finalize the production Phase 1 message templates and remaining read-only overlay data needed by the existing presentation.
5. Add `cargo audit` / `cargo deny` security CI after the dependency set stabilizes.
6. Inventory the live server and create the restricted CLN rune/credential material without changing production routes.
7. Deploy the org `clnaddress` fork and `lightning-goatsd` in canary/shadow mode.
8. Exercise `herd-canary@lightning-goats.com` and the OpenHAB test rule while LNbits remains authoritative.
9. Produce and execute the final controlled cutover only after canary acceptance.
