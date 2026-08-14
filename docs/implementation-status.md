# Phase 1 Implementation Status

This file tracks the current implementation state while the existing LNbits production stack remains authoritative.

## Cutover accounting decision

The standalone Lightning Goats ledger starts at **exactly zero** at production cutover.

There is no migration or synchronization of the old LNbits herd-wallet balance, and there is no late-LNbits invoice reconciliation. Pre-cutover LNbits state remains historical LNbits state.

The production accounting epoch is established by the explicitly initialized CLN `pay_index` cursor and the switch of `herd@lightning-goats.com` to the canonical `clnaddress` path. Only qualifying settlements with labels matching:

```text
clnaddress:v1:herd:<uuid>
```

can create feed credit in the new system.

Consequences:

- initial `feed_credit_sats = 0`;
- no LNbits wallet balance import;
- no LNbits invoice key in the new service stack;
- no pending-invoice allowlist;
- no migration timer or grace-period reconciler;
- an old LNbits invoice that settles after cutover does not create new Lightning Goats feed credit;
- the old LNbits database may be retained read-only for historical audit, but is outside the new accounting boundary.

## Canonical `clnaddress` dependency

The canonical Lightning Goats fork is:

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

The fork passed its inherited compatibility matrix across CLN 25.09.3, 25.12.1, 26.04, and 26.06.6, including Rust MSRV verification, Rust build/unit tests, and integration pytest.

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

## Release model

This is a single-operator deployment. Release automation should stay simple:

- tagged Linux binaries for `lightning-goatsd` and `lightning-goatsctl`;
- a compressed release archive;
- SHA-256 checksums;
- deployment configuration/systemd/nginx assets remain in Git and are installed manually.

`lightning-goats/clnaddress` already has an upstream-style binary release workflow and does not need an additional package format.

## Verification

The integrated Rust runtime, including durable overlay and Nostr/outbox workers, has been verified with:

```text
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

## Next

1. Finalize production Phase 1 message templates and remaining read-only overlay data needed by the existing presentation.
2. Add `cargo audit` / `cargo deny` security CI after the dependency set stabilizes.
3. Add tagged Linux binary releases and checksums for `lightning-goats`.
4. Inventory the live server and create the restricted CLN rune/credential material without changing production routes.
5. Deploy `lightning-goats/clnaddress` and `lightning-goatsd` in canary/shadow mode.
6. Exercise `herd-canary@lightning-goats.com` and the OpenHAB test rule while LNbits remains authoritative.
7. Initialize the production CLN cursor while the daemon is still in shadow mode.
8. Execute the zero-based cutover: disable old side effects, switch nginx Lightning Address routing to `clnaddress`, verify a real herd payment, then activate the new daemon.
9. Keep the old LNbits data read-only for audit until the operator chooses to archive/remove it; it has no accounting role after cutover.
