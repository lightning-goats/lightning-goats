# Lightning Goats Agent Guide

Production remains on HOLD under the 2026-09-08 audit. Begin deployment work with
`docs/deployment/audit-remediation.md` and `docs/deployment/deployment-artifacts.md`.
Passing artifact tests or earlier CI does not authorize production DNS/WireGuard
changes, real payments, or physical feeding. Source-level blockers remain open.

This repository is the standalone Lightning Goats payment-accounting, messaging, overlay, and feeder-automation service.

## Source of truth

Treat the GitHub Phase 1 tracker and the canonical documentation under `docs/` as the source of truth. Keep this file concise and update deeper docs when architecture or deployment decisions change.

Phase 1 tracker: https://github.com/lightning-goats/lightning-goats/issues/6

For a deployment/provisioning session, read first:

1. `docs/deployment/codex-handoff.md`
2. this `AGENTS.md`
3. `docs/implementation-status.md`
4. the supporting documents linked from the handoff

For implementation work, read in this order:

1. `docs/README.md`
2. `docs/planning/phase1-execution-plan.md`
3. `docs/architecture/phase1-strike-architecture.md`
4. `docs/architecture/lightning-address-registry.md`
5. `docs/security/phase1-threat-model.md`
6. `docs/security/openhab-feeder-gateway.md`
7. `docs/security/phase1-hardening-checklist.md`
8. `docs/deployment/codex-vps-bootstrap.md`
9. `docs/deployment/new-vps-staging.md`
10. `docs/testing/phase1-verification-matrix.md`
11. the GitHub issue being implemented

Before any production cutover work also read `docs/deployment/production-cutover.md`.

For future CyberHerd work read `docs/architecture/cyberherd-phase2-boundary.md`.

`docs/phase1-lnbits-rust-migration-plan.md` and CLN-specific portions of the old `docs/server-setup.md` are historical/superseded. Do not use them as the current execution plan.

## Phase 1 objective

Replace the production LNbits/Core Lightning payment path with a standalone `lightning-goatsd` architecture using Strike for Lightning receives while preserving durable feeder accounting, Nostr publishing, and the video overlay.

The physical feeder boundary is strengthened: `lightning-goatsd` talks only to a narrow in-house feeder gateway over the established WireGuard network. It does **not** hold an OpenHAB API token or directly invoke generic OpenHAB REST/rules.

Phase 1 does **not** implement CyberHerd membership/reward logic.

## Required Lightning Addresses

The configured Phase 1 registry contains:

- `herd@lightning-goats.com`
- `dexter@lightning-goats.com`
- `rowan@lightning-goats.com`
- `cosmo@lightning-goats.com`
- `newton@lightning-goats.com`
- `nova@lightning-goats.com`

All six credit the same `herd` feed-credit pool while preserving the actual paid `address_user` in durable state.

Nginx may route generic `/.well-known/lnurlp/<user>` paths to the service, but the application allowlist is authoritative. Unknown users must fail before any Strike API call.

## Security invariants

- `lightning-goatsd` must never require a spend-capable Strike credential in Phase 1.
- `lightning-goatsd` must never receive an OpenHAB API token.
- The OpenHAB project token belongs only to the trusted in-house feeder gateway.
- Do not reintroduce LNbits, Core Lightning, CLNRest, CLN `pay_index`, or `clnaddress` as production dependencies.
- A Strike webhook is a notification, not authoritative settlement data; verify it and fetch authoritative Strike state before crediting.
- Duplicate/replayed payment notifications must be idempotent.
- Payment settlement, feed-credit accounting, and durable `payment_received` event creation must remain atomic.
- Unknown Lightning Address users and invalid amounts must be rejected before provider contact.
- Public invoice creation must be rate-limited/backpressured; webhook method/body/content type must be constrained.
- Feeder actuation must remain serialized and ambiguity-safe.
- The feeder request UUID is the cross-boundary idempotency key. Duplicate/replayed UUIDs must never cause a second physical actuation.
- The existing correlated OpenHAB feeder owner remains the physical authority unless the operator explicitly approves replacing it.
- Gateway-local interval/hour caps and OpenHAB safety controls remain authoritative even if the VPS is compromised.
- Never automatically submit a fresh feeder actuation after an ambiguous outcome.
- Payment and feeder messages may publish to Nostr and the overlay. Informational/interface/weather messages are overlay-only and must never enter the Nostr outbox.
- Keep Nostr signing isolated through the existing `nak`/NIP-46 architecture; do not place a Nostr private key in application configuration or source control.
- Do not commit API keys, webhook secrets, OpenHAB tokens, WireGuard private keys, Nostr credentials, or other production secrets.
- Production runtime identities must not retain sudo/admin privileges.
- Production binaries/configuration must be root-owned and non-writable by runtime/deploy accounts.
- Treat DNS/domain control as payment-routing authority and follow the documented registrar/DNS hardening checklist.

## Durable event contract

Keep the ledger/event boundary backend-neutral and extensible for Phase 2. Phase 1 needs at least:

- `payment_received`
- `feeder_confirmed`
- informational/interface/weather-style overlay events

Settlement context must preserve at least provider/source identity, payment hash when available, `address_user`, `credit_pool`, amount, and settlement time.

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

## Feeder gateway

Follow `docs/security/openhab-feeder-gateway.md` and issue #17.

The VPS may access only the narrow feeder-gateway service over the approved WireGuard/UFW path. It must not directly reach generic OpenHAB REST/admin APIs or unrelated trusted-network services.

Reuse the established `10.8.0.0/24` WireGuard network. The trusted host is `10.8.0.6`; the old production hub remains `10.8.0.1` during staging. The new VPS gets a new keypair and an inventoried unused temporary `10.8.0.x` address. Host/UFW policy, not a new subnet, provides the application containment.

The gateway uses a dedicated OpenHAB USER/token and reuses the existing correlated feeder owner where its live request/result contract can be bound safely. It is not a generic proxy.

Any future CyberHerd feeder action must use the same durable feeder authority/gateway; do not create a bypass path.

## Deployment model

The replacement stack is built and tested on a new VPS in parallel with the current production VPS.

- Do not change production DNS until the new stack passes the Phase 1 verification matrix.
- Give the new VPS its own WireGuard identity during parallel testing.
- Do not run the same WireGuard private key on both old and new VPSes simultaneously.
- Reuse `10.8.0.0/24`; use an unused temporary staging address and never claim `10.8.0.1` while the old hub is active.
- Existing production WireGuard clients stay on the old VPS during staging; repoint them to the new VPS only during the operator-approved cutover.
- Keep the old VPS as a rollback/archive point until the new stack has been observed successfully in production.
- The public edge should be minimal: nginx/TLS, WireGuard, static site, and explicitly required application ingress.
- Restrict locally originated VPS traffic into the trusted network to `10.8.0.6` gateway ports explicitly required for the current stage.
- SSH must be key-only with direct root login disabled before production.

## Agent/server operations

Use a separate temporary Codex/deployment account (for example `lg-deploy`) for coding and host provisioning. The production daemon runs under a dedicated non-admin runtime identity (for example `lightning-goats`).

Production application units should be system-level systemd services using `User=lightning-goats`, not `systemctl --user` services under the Codex/deployment account.

The in-house feeder gateway should likewise run as a separate non-admin system service identity.

Capture privileged host changes in reproducible/reviewable repo scripts/configuration where practical.

Never load final production secrets while a development account still has unnecessary broad privileges if that can be avoided. Before production cutover, revoke/narrow temporary sudo, verify ownership/permissions, and perform a secret/access review. For maximum assurance the operator may choose to rebuild/reimage the final VPS from reviewed deployment artifacts.

Do not change production DNS, repoint production WireGuard clients, disable the old VPS, alter the operator-defined Strike balance ceiling/sweep policy, or activate real feeder side effects without an explicit operator-directed step.

## Rust and verification

- Preserve `#![forbid(unsafe_code)]`.
- Keep the pinned/locked dependency model.
- Run before completing code changes:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
```

Also run the repository security/audit gate documented in `docs/implementation-status.md` and the task-specific matrix in `docs/testing/phase1-verification-matrix.md`.

## Engineering style

- Prefer small explicit adapters over large SDK dependencies when practical.
- Fail closed around payment authority, feeder ambiguity, network reach, identity, and message-audience decisions.
- Keep financial state changes independent from best-effort presentation/publication failures.
- Preserve exact-event/idempotency semantics across restarts and retries.
- Reject invalid/unknown public input before invoking external providers.
- Keep physical safety constraints authoritative on the trusted side, not solely in public-VPS software.
- Update docs and tests in the same change when behavior or deployment contracts change.
