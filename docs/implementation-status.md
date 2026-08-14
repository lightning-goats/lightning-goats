# Phase 1 Implementation Status

This file tracks the current implementation state while the existing LNbits production stack remains authoritative.

## Implemented

- Rust 1.88 crate with `#![forbid(unsafe_code)]`.
- Strict `clnaddress:v1:<user>:<uuid>` invoice classifier.
- Durable SQLite database with WAL, `synchronous=FULL`, foreign keys, and migrations.
- Explicit one-time CLN `pay_index` cursor initialization.
- Atomic paid-invoice recording, herd crediting, and cursor advancement.
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
- Hardened `lightning-goatsd` systemd unit.
- Hardened `nak` NIP-46 bunker systemd unit and wrapper.
- nginx canary configuration example.
- server setup runbook.
- apply-ready initial `santyr/clnaddress` address-aware label patch.

## Previously verified green checkpoint

A prior integrated checkpoint passed:

- `cargo fmt --all --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-features`

on Rust 1.88. The durable overlay changes were added after that checkpoint and must pass the same gate before the Nostr/publication layer is considered ready.

## Next

1. Reconfirm CI on the durable overlay head.
2. Add Nostr signing through `nak` + NIP-46 client credential.
3. Add transactional signed-event outbox and retry of identical event IDs.
4. Port Phase 1 message templates/categories.
5. Add legacy LNbits opening-credit and late-settlement reconciliation tooling.
6. Extend `santyr/clnaddress` beyond the label patch with username hardening, per-address limits/comments, and file-based Zap receipt key loading.
7. Add `cargo audit` / `cargo deny` security CI after the dependency set stabilizes.
8. Run canary/shadow validation before any production cutover.
