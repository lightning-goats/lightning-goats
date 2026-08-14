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

The Phase 1 v1 contract defines:

```text
clnaddress:v1:<user>:<uuid>
```

and includes strict usernames, per-address min/max policy, LUD-12 comment validation, per-address Nostr policy, protected file-based Zap signer loading, atomic registry/cursor persistence, privacy hardening, and integration tests.

The fork passed its inherited compatibility matrix across CLN 25.09.3, 25.12.1, 26.04, and 26.06.6. Its release workflow publishes binary archives plus `SHA256SUMS`.

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
- Restricted CLNRest `waitanyinvoice` adapter using a systemd rune credential.
- CLNRest loopback-only enforcement, verified TLS with optional local CA, and proxy discovery disabled.
- OpenHAB override reader and automatic feeder adapter with proxy discovery disabled.
- Optional read-only OpenHAB ambient-temperature item for presentation status.
- `shadow` mode: observes/accounting only; no feeder actuation and no Nostr.
- `canary` mode: may invoke only the configured OpenHAB rule; Nostr remains disabled.
- `active` mode: production feeder plus NIP-46 Nostr processing.
- Ambiguous OpenHAB outcome fails to `unknown` and never automatically retries.
- Durable overlay event log.
- Payment/feed/error events committed atomically with the state changes they describe.
- Race-free overlay snapshot state and event sequence.
- Read-only `/ws/overlay` durable replay stream.
- Read-only `/healthz` and `/api/v1/status` endpoints.
- Status includes mode, feed credit, feeds due, remainder, unresolved feed ID, feeder override state, and optional temperature.
- Local operator CLI for cursor initialization/status/feed reconciliation.
- `nak` NIP-46 signing adapter.
- Shadow/canary-safe durable Nostr message cursor.
- Transactional signed-event outbox.
- Exact signed Nostr event retry rather than re-signing/re-IDing failed publications.
- Active-mode-only access to the NIP-46 client credential.
- Hardened production daemon and NIP-46 bunker systemd units.
- Hardened signer-free canary systemd unit with separate SQLite DB/port.
- Canary and production nginx configuration examples.
- Detailed server/canary/cutover runbook.
- `SECURITY.md` and weekly/PR `cargo audit` workflow.

## Overlay

The Phase 1 overlay lives in `lightning-goats/overlay` and has been ported to the standalone service contract:

- one WebSocket only: `wss://lightning-goats.com/ws/overlay`;
- read-only `/api/v1/status` for mode/override/temperature/fallback state;
- no LNbits WebSocket or API calls;
- no legacy FastAPI/CyberHerd API calls;
- durable sequence-gap detection and reconnect/resnapshot;
- multi-feed backlog and retained remainder display;
- payment QR/sats animation from `payment_received`;
- feeder animation **only** from committed `feeder_confirmed`;
- no browser-side feeder inference from the progress bar reaching 100%;
- no external GSAP dependency;
- `?canary=1` selects the isolated `/canary/...` endpoints;
- CyberHerd presentation intentionally deferred to Phase 2.

The overlay repository has CI that rejects reintroduction of legacy LNbits/CyberHerd runtime endpoints and enforces a single WebSocket construction path.

## Release model

This is a single-operator deployment. Tagged `lightning-goats` releases publish:

```text
lightning-goatsd
lightning-goatsctl
lightning-goats-<tag>-x86_64-linux-gnu.tar.gz
SHA256SUMS
```

Deployment configuration/systemd/nginx assets remain in Git and are installed manually.

## Verification gates

Required Rust gate:

```text
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo audit
```

The canary acceptance suite additionally requires real production-node testing with a harmless OpenHAB counter rule. The critical case is:

```text
2340 sats -> exactly 2 canary rule invocations -> 340 sats remainder -> 0 Nostr events
```

## Remaining before deployment

Code-side work should be considered complete when the current backend and overlay PRs are green and merged.

Then the remaining work is host-specific deployment/validation:

1. Inventory the live server and record exact CLN, CLNRest, LNbits, nginx, OpenHAB, `nak`, and existing service configuration.
2. Create/test the restricted CLN rune (`waitanyinvoice` / `listinvoices` only).
3. Set the real production and canary OpenHAB rule/item IDs in configuration.
4. Deploy the reviewed `clnaddress` and Lightning Goats binaries.
5. Configure `herd-canary@lightning-goats.com` plus the isolated canary nginx routes.
6. Initialize the canary cursor and run the full mainnet canary matrix with the harmless OpenHAB rule.
7. Prepare the NIP-46 bunker/client credentials and verify signing without public publication.
8. After canary acceptance, initialize a fresh production DB/cursor and run production ingress in `shadow`.
9. Execute the zero-based cutover with `FeederOverride=ON`, switch production Lightning Address routes, verify one real herd payment, then move to `active` and release the override.
10. Keep old LNbits data read-only for audit until the operator chooses to archive/remove it. It has no accounting role after cutover.

CyberHerd remains offline until Phase 2. No outbound Lightning spend capability belongs in Phase 1.
