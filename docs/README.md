# Lightning Goats Documentation

## Current Phase 1 source of truth

Phase 1 is the migration from LNbits/Core Lightning to the standalone Strike-backed Lightning Goats architecture tracked in GitHub issue #6 and its child issues.

Read these documents in this order:

1. [`../AGENTS.md`](../AGENTS.md) — concise agent invariants and navigation.
2. [`planning/phase1-execution-plan.md`](planning/phase1-execution-plan.md) — approved work order, milestones, and operator gates.
3. [`architecture/phase1-strike-architecture.md`](architecture/phase1-strike-architecture.md) — target runtime/data-flow architecture.
4. [`architecture/lightning-address-registry.md`](architecture/lightning-address-registry.md) — configured herd/goat addresses and shared feed-credit pool.
5. [`security/phase1-threat-model.md`](security/phase1-threat-model.md) — trust boundaries and privilege model.
6. [`security/openhab-feeder-gateway.md`](security/openhab-feeder-gateway.md) — required in-house OpenHAB/physical-feeder security boundary.
7. [`security/phase1-hardening-checklist.md`](security/phase1-hardening-checklist.md) — public edge, DNS, SSH, credentials, supply chain and operational value-at-risk controls.
8. [`deployment/codex-vps-bootstrap.md`](deployment/codex-vps-bootstrap.md) — how to prepare the new VPS and Codex/deploy account safely.
9. [`deployment/new-vps-staging.md`](deployment/new-vps-staging.md) — parallel staging procedure while the old VPS remains production.
10. [`testing/phase1-verification-matrix.md`](testing/phase1-verification-matrix.md) — executable functional/security acceptance matrix.
11. [`deployment/production-cutover.md`](deployment/production-cutover.md) — operator-gated DNS/WireGuard cutover and rollback procedure.
12. [`architecture/cyberherd-phase2-boundary.md`](architecture/cyberherd-phase2-boundary.md) — interfaces Phase 1 must preserve for future CyberHerd work.
13. [`implementation-status.md`](implementation-status.md) — current implementation/execution status.

## GitHub work queue

Core implementation:

- #6 — Phase 1 tracker
- #7 — backend-neutral payment/ledger domain
- #8 — receive-only Strike integration
- #9 — native Lightning Address/LNURL-pay
- #18 — configured `herd` + individual goat Lightning Address registry
- #10 — Phase 1 message templates
- #11 — Nostr + overlay event fan-out
- #12 — CyberHerd-ready boundaries
- #13 — remove CLN/LNbits runtime assumptions

Security/deployment:

- #17 — narrow in-house OpenHAB feeder gateway + dedicated WireGuard/UFW boundary
- #14 — new VPS/nginx/WireGuard/credential hardening
- #19 — domain/DNS, SSH, deployment provenance and operational Strike balance
- #20 — public LNURL/webhook abuse controls
- #15 — full verification matrix
- #16 — production cutover/rollback

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

## OpenHAB boundary

The public VPS does **not** receive an OpenHAB API token.

Physical feeder control is mediated by the in-house feeder gateway described in `security/openhab-feeder-gateway.md`. The trusted-side OpenHAB rule enforces duplicate UUID suppression and local safety gates even if the public VPS is compromised.

## Legacy/historical documents

The following documents describe the **previous, superseded CLN/`clnaddress` migration architecture** and must not be used as the execution plan for the current Phase 1:

- `phase1-lnbits-rust-migration-plan.md`
- `server-setup.md` where it assumes CLNRest/`clnaddress` production ingress or direct legacy OpenHAB integration

They are retained temporarily as historical design context and may contain useful feeder/Nostr/overlay implementation rationale. When they conflict with the documents listed under **Current Phase 1 source of truth**, the current Strike-backed documentation wins.

## Phase 2

CyberHerd business logic is not part of Phase 1. See `architecture/cyberherd-phase2-boundary.md` for the intended seam and future service/module options.

Future CyberHerd functionality must not bypass the feeder gateway or expand the receive-only `lightning-goatsd` credential into spend authority by default.
