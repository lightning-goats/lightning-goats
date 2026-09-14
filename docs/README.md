# Lightning Goats Documentation

## Current plan: parallel live pilot

**Operator decision, 2026-09-14:** get `herd@feeder.lightning-goats.com` running on
the new VPS alongside the old system, send real payments, observe the results and
fix actual problems. The operator decides when to redirect established addresses,
DNS, Nostr profile metadata and other public references.

Start with [Parallel live pilot](deployment/parallel-live-pilot.md). It supersedes
blanket HOLD/full-audit/sandbox-first launch restrictions in older documentation,
including host handoffs and audit addenda. They remain implementation references;
old unperformed checks do not become passing evidence. This is not a statement
that services are already installed or live.

## Short execution path

1. [Agent guide](../AGENTS.md) and
   [execution plan](planning/phase1-execution-plan.md): priorities and boundaries.
2. [New VPS pilot runbook](deployment/new-vps-staging.md): dedicated hostname,
   existing daemon/gateway and real manual payments.
3. [Verification matrix](testing/phase1-verification-matrix.md): small initial
   preflight, checks during the pilot, follow-up work rather than one giant gate.
4. [Cutover and rollback](deployment/production-cutover.md): operator-directed
   public address switch after the pilot looks right; WireGuard can move separately.

## Technical references, not prerequisite reading projects

- [Strike architecture](architecture/phase1-strike-architecture.md) and
  [address registry](architecture/lightning-address-registry.md).
- [Gateway boundary](security/openhab-feeder-gateway.md),
  [weather/overlay](architecture/weather-overlay.md) and
  [WireGuard topology](deployment/wireguard-topology.md).
- HOME: [gateway handoff](deployment/home-gateway-agent-handoff.md).
- VPS: [remediation handoff](deployment/new-vps-remediation-handoff.md).
- [Deployment artifacts](deployment/deployment-artifacts.md),
  [audit findings](deployment/audit-remediation.md),
  [threat model](security/phase1-threat-model.md),
  [hardening checklist](security/phase1-hardening-checklist.md) and
  [implementation history](implementation-status.md).

Apply the pilot plan's priority/scope classification before treating language in
these references as a launch blocker. There is no requirement to close every issue,
complete every host rehearsal, or obtain Strike sandbox access before manual pilot
payments. Do not ignore an actual defect in the feature being used.

## Addresses and physical ownership

Initially publish **herd@feeder.lightning-goats.com** only. The daemon still keeps
its six required configured users; the pilot edge restricts discovery/callbacks to
herd. The eventual addresses remain `herd`, `dexter`, `rowan`, `cosmo`, `newton` and
`nova` at `lightning-goats.com`, sharing the herd credit pool and retaining recipient
attribution. No registry rewrite is needed to begin.

Old and new sites/payment services can coexist. Both must respect one local feeder
owner and its interval/cap/duplicate controls, or only one feeder-dispatch path may
be enabled at a time. Keep separate ledgers and retain real pilot credits at cutover.
Never copy the OpenHAB token to the VPS or bypass the gateway. Weather stays overlay-only.
The old WireGuard hub remains `10.8.0.1` during the pilot.

## Tracking and history

#6 is the migration tracker; #15 records what the pilot actually demonstrates;
#16 is the eventual cutover; #17 and #21 track gateway and weather details. Leave
unverified items open, but do not treat an open parent issue as a ban on pilot work.

The former exhaustive execution/staging/matrix texts remain in Git history at
`df6be90f1b0505d183643ec55a298b382cd43b6c`. Audit evidence and regression tests are
retained. Historical CLN/clnaddress plans (`phase1-lnbits-rust-migration-plan.md` and
CLN-specific `server-setup.md`) are not the current runtime architecture.

[CyberHerd Phase 2](architecture/cyberherd-phase2-boundary.md), alternative backends,
website expansion and optional tooling are not dependencies of this pilot.
