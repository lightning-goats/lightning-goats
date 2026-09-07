# Lightning Goats Documentation

## Current Phase 1 source of truth

Phase 1 replaces the production LNbits/Core Lightning payment path with the standalone Strike-backed Lightning Goats architecture tracked in GitHub issue #6.

For **deployment**, start with:

1. [`../AGENTS.md`](../AGENTS.md) — concise invariants and agent navigation.
2. [`deployment/codex-handoff.md`](deployment/codex-handoff.md) — complete Codex staging/deployment handoff and operator gates.
3. [`implementation-status.md`](implementation-status.md) — current software/deployment readiness.
4. [`security/phase1-threat-model.md`](security/phase1-threat-model.md) — trust boundaries and privilege model.
5. [`security/openhab-feeder-gateway.md`](security/openhab-feeder-gateway.md) — trusted OpenHAB/weather/physical-feeder boundary.
6. [`security/phase1-hardening-checklist.md`](security/phase1-hardening-checklist.md) — public edge, DNS, SSH, credentials and value-at-risk controls.
7. [`deployment/wireguard-topology.md`](deployment/wireguard-topology.md) — established `10.8.0.0/24` topology and hub migration.
8. [`deployment/new-vps-staging.md`](deployment/new-vps-staging.md) — parallel staging while old VPS remains production.
9. [`deployment/public-site-migration.md`](deployment/public-site-migration.md) — migration/simplification of `lightning-goats.com`.
10. [`testing/phase1-verification-matrix.md`](testing/phase1-verification-matrix.md) — functional/security acceptance matrix.
11. [`deployment/production-cutover.md`](deployment/production-cutover.md) — operator-gated cutover and rollback.

For implementation/architecture context also read:

- [`planning/phase1-execution-plan.md`](planning/phase1-execution-plan.md)
- [`architecture/phase1-strike-architecture.md`](architecture/phase1-strike-architecture.md)
- [`architecture/lightning-address-registry.md`](architecture/lightning-address-registry.md)
- [`architecture/messaging-phase1.md`](architecture/messaging-phase1.md)
- [`architecture/weather-overlay.md`](architecture/weather-overlay.md)
- [`architecture/cyberherd-phase2-boundary.md`](architecture/cyberherd-phase2-boundary.md)
- [`deployment/codex-vps-bootstrap.md`](deployment/codex-vps-bootstrap.md)

## Phase 1 work map

Core implementation:

- #6 — Phase 1 tracker
- #7 — backend-neutral payment/ledger domain
- #8 — receive-only Strike integration
- #9 — native Lightning Address/LNURL-pay
- #18 — configured herd + individual goat address registry
- #10 — Phase 1 message templates
- #11 — Nostr + overlay event fan-out
- #17 — trusted OpenHAB feeder gateway
- #21 — weather overlay compatibility through the gateway
- #13 — remove CLN/LNbits runtime assumptions
- #12 — CyberHerd-ready boundaries

Deployment/security:

- #26 — migrate/simplify public `lightning-goats.com` site
- #14 — new VPS/nginx/WireGuard/credential hardening
- #19 — domain/DNS, SSH, deployment provenance and operational Strike balance
- #20 — public LNURL/webhook abuse controls
- #15 — full verification matrix
- #16 — production cutover/rollback

## Network topology

```text
10.8.0.0/24

10.8.0.1   current production VPS / WireGuard hub during staging
10.8.0.6   in-house OpenHAB + weather + trusted gateway host
```

The new VPS gets a new WireGuard keypair and an inventoried unused temporary `10.8.0.x` address. It must not claim `10.8.0.1` until the old hub is stopped during operator-approved cutover.

The security boundary is source-specific firewalling on the established network: the VPS may reach the trusted Lightning Goats gateway, not generic OpenHAB/weather/SSH/Postgres/LAN services.

## Phase 1 Lightning Addresses

```text
herd@lightning-goats.com
dexter@lightning-goats.com
rowan@lightning-goats.com
cosmo@lightning-goats.com
newton@lightning-goats.com
nova@lightning-goats.com
```

All six credit the same `herd` feeder pool while preserving recipient metadata. Nginx may route generic LNURL paths, but the application registry is authoritative and rejects unconfigured users before provider contact.

## Trusted OpenHAB/weather boundary

The public VPS has no OpenHAB API token and no direct access to the legacy weather receiver at `10.8.0.6:5000`.

`lightning-goats-gateway` on `10.8.0.6`:

- owns the dedicated OpenHAB integration token;
- reuses the existing correlated feeder-owner request/result contract;
- persists feeder UUID state and never automatically resends an ambiguous UUID;
- adds local minimum-interval/feeds-per-hour safety limits;
- reads weather only from local `/get_received_data`;
- exposes the narrow gateway API to the VPS.

Weather/interface messages are overlay-only.

## Deployment preflights

After installing the reviewed artifacts, run:

```sh
# on 10.8.0.6
bash deploy/checks/gateway-preflight.sh

# on the new VPS
bash deploy/checks/vps-preflight.sh
```

Neither script commands the physical feeder.

## Historical documents

`phase1-lnbits-rust-migration-plan.md` and CLN-specific portions of `server-setup.md` describe the superseded CLN/`clnaddress` architecture. They are historical context only and must not override the documents above.

## Phase 2

CyberHerd membership/headbutts/rewards/NIP-05 and outbound payment authority are not Phase 1. See `architecture/cyberherd-phase2-boundary.md`. Future CyberHerd functionality must use the same durable payment/feeder boundaries and must not bypass the trusted gateway.
