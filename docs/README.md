# Lightning Goats Documentation

**Production HOLD (2026-09-08 audit, carried forward 2026-09-10).** Earlier
completion claims do not establish deployment readiness. Read
[`deployment/audit-remediation.md`](deployment/audit-remediation.md) and the
[`deployment artifact guide`](deployment/deployment-artifacts.md) first.
The new VPS agent should follow
[`new-vps-remediation-handoff.md`](deployment/new-vps-remediation-handoff.md).

## Current Phase 1 source of truth

Phase 1 is the migration from LNbits/Core Lightning to the standalone Strike-backed Lightning Goats architecture tracked in GitHub issue #6 and its child issues.

Read these documents in this order:

1. [`../AGENTS.md`](../AGENTS.md) — concise agent invariants and navigation.
2. [`planning/phase1-execution-plan.md`](planning/phase1-execution-plan.md) — approved work order, milestones, and operator gates.
3. [`architecture/phase1-strike-architecture.md`](architecture/phase1-strike-architecture.md) — target runtime/data-flow architecture.
4. [`architecture/lightning-address-registry.md`](architecture/lightning-address-registry.md) — configured herd/goat addresses and shared feed-credit pool.
5. [`architecture/weather-overlay.md`](architecture/weather-overlay.md) — legacy weather compatibility and sanitized overlay-only integration.
6. [`security/phase1-threat-model.md`](security/phase1-threat-model.md) — trust boundaries and privilege model.
7. [`security/openhab-feeder-gateway.md`](security/openhab-feeder-gateway.md) — required in-house OpenHAB/weather/physical-feeder security boundary.
8. [`security/phase1-hardening-checklist.md`](security/phase1-hardening-checklist.md) — public edge, DNS, SSH, credentials, supply chain and operational value-at-risk controls.
9. [`deployment/wireguard-topology.md`](deployment/wireguard-topology.md) — existing `10.8.0.0/24` topology, staging peer, and hub cutover plan.
10. [`deployment/codex-vps-bootstrap.md`](deployment/codex-vps-bootstrap.md) — how to prepare the new VPS and Codex/deploy account safely.
11. [`deployment/new-vps-staging.md`](deployment/new-vps-staging.md) — parallel staging procedure while the old VPS remains production.
12. [`testing/phase1-verification-matrix.md`](testing/phase1-verification-matrix.md) — executable functional/security acceptance matrix.
13. [`deployment/production-cutover.md`](deployment/production-cutover.md) — operator-gated DNS/WireGuard cutover and rollback procedure.
14. [`architecture/cyberherd-phase2-boundary.md`](architecture/cyberherd-phase2-boundary.md) — interfaces Phase 1 must preserve for future CyberHerd work.
15. [`implementation-status.md`](implementation-status.md) — current implementation/execution status.

## GitHub work queue

Core implementation:

- #6 — Phase 1 tracker
- #7 — backend-neutral payment/ledger domain
- #8 — receive-only Strike integration
- #9 — native Lightning Address/LNURL-pay
- #18 — configured `herd` + individual goat Lightning Address registry
- #10 — Phase 1 message templates
- #11 — Nostr + overlay event fan-out
- #21 — weather overlay compatibility through the in-house gateway
- #12 — CyberHerd-ready boundaries
- #13 — remove CLN/LNbits runtime assumptions

Security/deployment:

- #17 — narrow in-house OpenHAB/weather feeder gateway + WireGuard/UFW boundary
- #14 — new VPS/nginx/WireGuard/credential hardening
- #19 — domain/DNS, SSH, deployment provenance and operational Strike balance
- #20 — public LNURL/webhook abuse controls
- #15 — full verification matrix
- #16 — production cutover/rollback

## Existing WireGuard topology

The project already uses:

```text
10.8.0.0/24
```

Known Phase 1 nodes:

```text
10.8.0.1   current production VPS / WireGuard hub
10.8.0.6   in-house OpenHAB + weather host
```

The new VPS gets a new WireGuard keypair and an unused temporary `10.8.0.x` address during staging. It must not claim `10.8.0.1` until the old hub is stopped during the operator-approved cutover. See `deployment/wireguard-topology.md`.

## Phase 1 Lightning Addresses

Required configured addresses:

```text
herd@lightning-goats.com
dexter@lightning-goats.com
rowan@lightning-goats.com
cosmo@lightning-goats.com
newton@lightning-goats.com
nova@lightning-goats.com
```

All six use the same Phase 1 feeder credit pool while preserving recipient metadata. Nginx may route generic LNURL paths to the service, but arbitrary unconfigured users are not payable.

## OpenHAB/weather boundary

The public VPS does **not** receive an OpenHAB API token and does not connect directly to the legacy weather service at `10.8.0.6:5000`.

Physical feeder control and weather/status reads are mediated by the in-house Lightning Goats integration gateway described in `security/openhab-feeder-gateway.md`.

The gateway:

- owns the dedicated OpenHAB integration credential;
- implements the duplicate-safe feeder UUID request/ack path;
- enforces/observes local OpenHAB safety gates;
- reads legacy weather data locally from `/get_received_data`;
- exposes only a sanitized read-only `/v1/weather` contract to `lightning-goatsd`.

Weather messages are overlay-only.

## Legacy/historical documents

The following documents describe the **previous, superseded CLN/`clnaddress` migration architecture** and must not be used as the execution plan for the current Phase 1:

- `phase1-lnbits-rust-migration-plan.md`
- `server-setup.md` where it assumes CLNRest/`clnaddress` production ingress or direct legacy OpenHAB integration

They are retained temporarily as historical design context and may contain useful feeder/Nostr/overlay implementation rationale. When they conflict with the documents listed under **Current Phase 1 source of truth**, the current Strike-backed documentation wins.

## Phase 2

CyberHerd business logic is not part of Phase 1. See `architecture/cyberherd-phase2-boundary.md` for the intended seam and future service/module options.

Future CyberHerd functionality must not bypass the in-house integration gateway or expand the receive-only `lightning-goatsd` credential into spend authority by default.
