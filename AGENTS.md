# Lightning Goats Agent Guide

This repository contains the standalone Strike-backed Lightning Goats payment, messaging, overlay, and feeder-automation stack.

Phase 1 tracker: https://github.com/lightning-goats/lightning-goats/issues/6

## Start here

For deployment/provisioning work, read in this order:

1. `docs/deployment/codex-handoff.md`
2. `docs/README.md`
3. `docs/implementation-status.md`
4. `docs/security/phase1-threat-model.md`
5. `docs/security/openhab-feeder-gateway.md`
6. `docs/security/phase1-hardening-checklist.md`
7. `docs/deployment/wireguard-topology.md`
8. `docs/testing/phase1-verification-matrix.md`
9. `docs/deployment/production-cutover.md`

For architecture work also read:

- `docs/architecture/phase1-strike-architecture.md`
- `docs/architecture/lightning-address-registry.md`
- `docs/architecture/messaging-phase1.md`
- `docs/architecture/weather-overlay.md`
- `docs/architecture/cyberherd-phase2-boundary.md`

CLN/LNbits migration documents are historical. Do not use them as the current execution plan.

## Final Phase 1 boundary

`lightning-goatsd` on the new VPS uses:

- Strike receive/read-only authority;
- native LNURL-pay / Lightning Addresses;
- durable SQLite accounting;
- NIP-46/`nak` Nostr publication;
- the overlay websocket;
- one narrow `GatewayClient` to the trusted in-house gateway.

It does **not** use LNbits, Core Lightning/CLNRest, `clnaddress`, a CLN rune, an OpenHAB token, or a spend-capable Strike key.

The trusted `lightning-goats-gateway` runs on `10.8.0.6`, owns the dedicated OpenHAB USER token, reuses the existing correlated feeder-owner contract, exposes sanitized weather, and enforces additional local feed-rate limits.

## Required Lightning Addresses

All six are mandatory and credit the common `herd` pool while preserving `address_user`:

- `herd@lightning-goats.com`
- `dexter@lightning-goats.com`
- `rowan@lightning-goats.com`
- `cosmo@lightning-goats.com`
- `newton@lightning-goats.com`
- `nova@lightning-goats.com`

Unknown users and invalid amounts must fail before any Strike call.

## Non-negotiable security invariants

- Strike webhooks are notifications; verify HMAC then fetch authoritative Strike state before crediting.
- Settlement and feed-credit/event creation remain exactly-once and durable.
- `lightning-goatsd` never receives an OpenHAB credential.
- The feeder-attempt UUID is the cross-boundary idempotency key; an ambiguous/pending UUID is never automatically re-actuated.
- Existing OpenHAB physical-owner safety remains authoritative; the gateway adds, rather than replaces, local safety limits.
- `FeederOverride` and `LightningGoatsRemoteEnabled` fail closed.
- Payment/feed messages may go to Nostr + overlay; interface/weather messages are overlay-only.
- Keep Nostr private signing authority outside the daemon through NIP-46; never place an nsec in source/config/browser code.
- Never commit API keys, webhook secrets, OpenHAB tokens, WireGuard private keys, or Nostr credentials.
- Production binaries/config/static files are root-owned and non-writable by runtime/deploy users.
- Production runtime identities have no sudo/admin privileges.
- DNS/domain control is payment-routing authority and must receive account-level hardening.

## Network topology

Reuse the established `10.8.0.0/24` WireGuard network:

- old production VPS/hub: `10.8.0.1` during staging;
- trusted OpenHAB/weather host: `10.8.0.6`;
- new VPS: new keypair + inventoried unused temporary `10.8.0.x` during parallel staging.

Do not reuse the old VPS private key during staging. Do not assign `10.8.0.1` to the new VPS until the old hub is stopped. Enforce the Lightning Goats trusted-side capability with source-specific UFW rules: VPS -> `10.8.0.6:8789` only (plus isolated canary port while staging), not generic LAN/OpenHAB/weather access.

## Agent/server identities

Use a separate deployment/Codex account such as `lg-deploy` with temporary staging sudo. Run production services as system-level systemd units under dedicated non-admin accounts (`lightning-goats`, `lightning-goats-gateway`).

Before final production credentials/cutover, audit privileged changes and revoke/narrow broad deployment sudo. For maximum assurance, rebuild from reviewed artifacts before introducing final secrets.

## Operator gates

Do not, without explicit operator direction:

- stop the old production WireGuard hub;
- move `10.8.0.1` to the new VPS or repoint production peers;
- change production DNS;
- enable a real physical feeder canary;
- change feeder safety limits or the Strike operational-balance ceiling;
- introduce spend-capable Strike authority;
- destroy the old VPS/backups/CLN recovery material.

## Verification

Preserve `#![forbid(unsafe_code)]` and the locked dependency model. Before code completion run:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
cargo tree -i rsa --locked
cargo audit --ignore RUSTSEC-2023-0071
```

The `RUSTSEC-2023-0071` exception is acceptable only while `rsa` is unreachable from the active application dependency graph, as documented in `docs/implementation-status.md`.

For deployment, run the appropriate preflight scripts and the complete `docs/testing/phase1-verification-matrix.md`. Neither preflight performs a physical feeder action.
