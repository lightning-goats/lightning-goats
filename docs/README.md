# Lightning Goats Documentation

## Current Phase 1 source of truth

Phase 1 is the migration from LNbits/Core Lightning to the standalone Strike-backed Lightning Goats architecture tracked in GitHub issue #6 and child issues #7–#16.

Read these documents in this order:

1. [`../AGENTS.md`](../AGENTS.md) — concise agent invariants and navigation.
2. [`planning/phase1-execution-plan.md`](planning/phase1-execution-plan.md) — approved work order, milestones, and operator gates.
3. [`architecture/phase1-strike-architecture.md`](architecture/phase1-strike-architecture.md) — target runtime/data-flow architecture.
4. [`security/phase1-threat-model.md`](security/phase1-threat-model.md) — trust boundaries, least privilege, credentials, and network containment.
5. [`deployment/codex-vps-bootstrap.md`](deployment/codex-vps-bootstrap.md) — how to prepare the new VPS and Codex/deploy account safely.
6. [`deployment/new-vps-staging.md`](deployment/new-vps-staging.md) — parallel staging procedure while the old VPS remains production.
7. [`deployment/production-cutover.md`](deployment/production-cutover.md) — operator-gated DNS/WireGuard cutover and rollback procedure.
8. [`architecture/cyberherd-phase2-boundary.md`](architecture/cyberherd-phase2-boundary.md) — interfaces Phase 1 must preserve for future CyberHerd work.
9. [`implementation-status.md`](implementation-status.md) — current implementation/execution status.

## GitHub work queue

- #6 — Phase 1 tracker
- #7 — backend-neutral payment/ledger domain
- #8 — receive-only Strike integration
- #9 — native Lightning Address/LNURL-pay
- #10 — Phase 1 message templates
- #11 — Nostr + overlay event fan-out
- #12 — CyberHerd-ready boundaries
- #13 — remove CLN/LNbits runtime assumptions
- #14 — new VPS/nginx/WireGuard/credential hardening
- #15 — full verification matrix
- #16 — production cutover/rollback

## Legacy/historical documents

The following documents describe the **previous, superseded CLN/`clnaddress` migration architecture** and must not be used as the execution plan for the current Phase 1:

- `phase1-lnbits-rust-migration-plan.md`
- `server-setup.md` where it assumes CLNRest/`clnaddress` production ingress

They are retained temporarily as historical design context and may contain useful feeder/Nostr/overlay implementation rationale. When they conflict with the documents listed under **Current Phase 1 source of truth**, the current Strike-backed documentation wins.

## Phase 2

CyberHerd business logic is not part of Phase 1. See `architecture/cyberherd-phase2-boundary.md` for the intended seam and future service/module options.
