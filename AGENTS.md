# Lightning Goats Agent Guide

This repository is the standalone Lightning Goats payment-accounting, messaging, overlay, and feeder-automation service.

## Source of truth

Treat the GitHub Phase 1 tracker and the repository documentation under `docs/` as the source of truth. Keep this file concise and update the deeper docs when architecture or deployment decisions change.

Phase 1 tracker: https://github.com/lightning-goats/lightning-goats/issues/6

## Phase 1 objective

Replace the production LNbits/Core Lightning payment path with a standalone `lightning-goatsd` architecture using Strike for Lightning receives while preserving durable feeder accounting, OpenHAB safety controls, Nostr publishing, and the video overlay.

Phase 1 does **not** implement CyberHerd membership/reward logic.

## Security invariants

- `lightning-goatsd` must never require a spend-capable Strike credential in Phase 1.
- Do not reintroduce LNbits, Core Lightning, CLNRest, CLN `pay_index`, or `clnaddress` as production dependencies.
- A Strike webhook is a notification, not authoritative settlement data; verify the webhook and fetch authoritative Strike state before crediting.
- Duplicate/replayed payment notifications must be idempotent.
- Payment settlement, feed-credit accounting, and durable `payment_received` event creation must remain atomic.
- Feeder actuation must remain serialized and ambiguity-safe. Never automatically retry an OpenHAB actuation whose physical outcome is unknown.
- `FeederOverride` remains a fail-safe gate.
- Payment and feeder messages may publish to Nostr and the overlay. Informational/interface/weather messages are overlay-only and must never enter the Nostr outbox.
- Keep Nostr signing isolated through the existing `nak`/NIP-46 architecture; do not place a Nostr private key in application configuration or source control.
- Do not commit API keys, webhook secrets, OpenHAB tokens, WireGuard private keys, Nostr credentials, or other production secrets.
- Production runtime identities must not retain sudo/admin privileges.

## Durable event contract

Keep the ledger/event boundary backend-neutral and extensible for Phase 2. Phase 1 needs at least:

- `payment_received`
- `feeder_confirmed`
- informational/interface/weather-style overlay events

Future CyberHerd producers must be able to add new event types without redesigning Strike settlement or feeder accounting.

## Messaging

Phase 1 ports only these existing template categories:

- `sats_received`
- `feeder_trigger`
- informational/interface/weather-style overlay messages

Canonical legacy behavior lives in:

- `lightning-goats/cyberherd_messaging`
- `lightning-goats/lightning_goats_extension`
- `lightning-goats/cyberherd_extension`

Use safe simple-placeholder rendering only. Keep Nostr and overlay render targets distinct where presentation differs.

## Deployment model

The replacement stack is built and tested on a new VPS in parallel with the current production VPS.

- Do not change production DNS until the new stack passes the Phase 1 verification matrix.
- Give the new VPS its own WireGuard identity during parallel testing.
- Do not run the same WireGuard private key on both old and new VPSes simultaneously.
- Keep the old VPS as a rollback/archive point until the new stack has been observed successfully in production.
- The public edge should be minimal: nginx/TLS, WireGuard as required, static site, and explicitly required application ingress.
- Restrict the VPS WireGuard peer at the trusted side to only explicitly required hosts/ports.

## Agent/server operations

Prefer a separate temporary Codex/deployment account for coding and host provisioning. The production daemon should run under a dedicated non-admin runtime identity.

Never load production secrets while a development account still has unnecessary broad privileges if that can be avoided. Before production cutover, revoke temporary sudo, verify ownership/permissions, and perform a secret/access review.

Do not change production DNS, production WireGuard peer identity, or activate real feeder side effects without an explicit operator-directed cutover step.

## Rust and verification

- Preserve `#![forbid(unsafe_code)]`.
- Keep the pinned/locked dependency model.
- Run before completing code changes:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
```

Also run the repository security/audit gate documented in `docs/implementation-status.md` and any task-specific integration tests.

## Engineering style

- Prefer small explicit adapters over large SDK dependencies when practical.
- Fail closed around payment authority, feeder ambiguity, identity, and message-audience decisions.
- Keep financial state changes independent from best-effort presentation/publication failures.
- Preserve exact-event/idempotency semantics across restarts and retries.
- Update docs and tests in the same change when behavior or deployment contracts change.
