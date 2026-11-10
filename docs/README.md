# Lightning Goats documentation

The operator-directed CyberHerd port is now tracked in
[`planning/cyberherd-implementation-plan.md`](planning/cyberherd-implementation-plan.md),
including the updated explanation/code references and the explicit no-NIP-05 and
confirmed-feeding reward rules. That request supersedes the older CyberHerd
deferral in the historical delivery order below.

The optional [read-only CyberHerd roster and OBS state](architecture/cyberherd-public-roster.md)
are disabled by default and add no financial or operator mutation capability.
The approved current rollout now serves the [live leaderboard](testing/cyberherd-live-readonly-site-20261110.md),
with operator controls, CyberHerd Nostr and payouts still disabled.
Optional [Nostr sign-in and operator controls](architecture/cyberherd-operator-interface.md)
remain separately gated and uninstalled; their [source checkpoint](testing/cyberherd-operator-checkpoint-20261110.md)
grants no live financial or physical activation.

[Explicit failed-payment cost review](architecture/cyberherd-failed-cost-recovery.md)
now has a [485-test source checkpoint](testing/cyberherd-failed-cost-checkpoint-20261110.md).
Provider failure alone never releases funds, and the legacy two-store restore
helper refuses CyberHerd schemas. Whole-stack recovery and installation remain
unfinished; no financial review or payout is activated by the source checkpoint.

The [website WebLN/chat release](testing/website-payment-chat-checkpoint-20261110.md) is live with an invisible QR-sized target and authenticated zap display. The [private contact source](testing/website-contact-checkpoint-20261110.md) implements free Nostr DMs; the separate email contact fee remains unfinished.

The [SQLite backport](security/sqlite-binding-backport.md) preserves Rust 1.88
and SQLx 0.8.6 while using the patched engine. Its
[486-test checkpoint](testing/cyberherd-sqlite-checkpoint-20261110.md) passes the
dependency audit and native engine check. It does not update installed programs.

Native [four-journal maintenance](deployment/cyberherd-state-maintenance.md)
provides offline snapshots, retention reports and startup-blocked restore staging.
Its [491-test checkpoint](testing/cyberherd-state-maintenance-checkpoint-20261110.md)
records verified source and inactive artifacts, without financial resumption authority.

The [overlay-stage preparer](deployment/cyberherd-overlay-stage.md) freezes
native-zap, membership and loopback-view configuration for review. It preserves
the existing receive/feed behavior, keeps payout ceilings zero and omits a
membership Nostr signer. It implements no activation or service changes.

The separate [maintenance executor](deployment/cyberherd-overlay-maintenance.md)
now has a passing [installed rehearsal and VPS execution plan](testing/cyberherd-overlay-maintenance-checkpoint-20261110.md).
Its native backup/apply/resume/configuration-only rollback ran on synthetic
state in isolated systemd services. The approved production attempt verified its backup, then refused an inherited
timeout policy before startup. Recovery restored the original core. A bounded
compatibility correction subsequently passed exact qualification, and a fresh
approved transaction successfully started membership/overlay intake. The
[live checkpoint](testing/cyberherd-live-overlay-stage-20261110.md) records the
current native backup, six zap-capable addresses and public OBS roster readback.
CyberHerd Nostr notes, rewards and payouts remain disabled.

Fresh [offline journal initialization](deployment/cyberherd-offline-initialization.md)
now has a [492-test source checkpoint](testing/cyberherd-bootstrap-checkpoint-20261110.md),
including mandatory native subprocess checks under three distinct runtime UIDs.
It uses no credentials and does not install or activate services.

The [494-test checkpoint](testing/cyberherd-core-upgrade-checkpoint-20261110.md)
qualifies [native legacy-core backup and migration comparison](deployment/cyberherd-core-upgrade.md)
on separate copies. The actual paid-core upgrade transaction remains required.

The [495-test checkpoint](testing/cyberherd-installation-checkpoint-20261110.md)
also qualifies the [fresh disabled-role installer](deployment/cyberherd-inactive-installation.md)
on a disposable systemd VM, with actual separate identities/masks/native journals.
The live host and financial activation remain unchanged.

The native incoming zap/receipt contract is described in
[`architecture/native-nip57.md`](architecture/native-nip57.md). It preserves
ordinary LNURL payments and requires explicit feature activation.

The [separate CyberHerd intake worker](architecture/cyberherd-worker.md) now
connects core events, authenticated discovery and ordered membership in source.
It now runs in the approved overlay-only stage, with no provider-spend or
membership Nostr publication capability enabled. Optional
[reward planning](architecture/cyberherd-reward-planning.md) freezes eligible
confirmed-feeding plans before obtaining bounded core reservations.

The operator-directed 2026-11-08 Strike restoration is recorded in
[`deployment/strike-receive-restoration.md`](deployment/strike-receive-restoration.md),
including exact API scopes, webhook staging, WAF and payment-only deployment.
The operator-supplied OBS layout and its current data/QR contracts are documented
in [`deployment/obs-overlay-restoration.md`](deployment/obs-overlay-restoration.md).
Local CLI grants and the 10-sat message threshold are documented in
[`deployment/operator-credits.md`](deployment/operator-credits.md).
Paused credit retention, queued-feed messaging and confirmation-only debits are
documented in [`deployment/paused-feeding-queue.md`](deployment/paused-feeding-queue.md).
Household NIP-46 client preparation and the remaining signer/activation steps are
recorded in [`deployment/household-nostr-bunker.md`](deployment/household-nostr-bunker.md).

## Current delivery order — 2026-09-17

Start with [the delivery roadmap](planning/delivery-roadmap.md):
**minimal Strike/Lightning first, Square fiat Feedings second, Monero on hold until
the operator explicitly resumes it**. The prepared #97 work is preserved in draft
PR #107, not a dependency of either active deliverable. Do not undo already merged
accounting/migrations or reset paid state to pause an unused rail.

[The parallel live pilot](deployment/parallel-live-pilot.md) remains the practical
path for `herd@feeder.lightning-goats.com` alongside the old system. The operator
sends manual payments, observes feeding and chooses the eventual established-address,
DNS and Nostr-profile cutover. Older blanket HOLD/full-audit/sandbox-first language
is historical, not a veto on unrelated safe pilot work. No plan is a claim that
services are already installed or that live acceptance has happened.

## Short execution path

1. [Agent guide](../AGENTS.md), [delivery roadmap](planning/delivery-roadmap.md) and
   [Lightning execution plan](planning/phase1-execution-plan.md).
2. [VPS pilot runbook](deployment/new-vps-staging.md), with the required current
   Lightning WAF work tracked in [#106](https://github.com/lightning-goats/lightning-goats/issues/106).
3. [Verification matrix](testing/phase1-verification-matrix.md): selected-source
   and actual pilot evidence, not a requirement to finish every future feature.
4. [Cutover and rollback](deployment/production-cutover.md): explicit operator
   public-address switch with all real paid state preserved.

Minimal Lightning includes real Nostr payment/feed messages and the actual OBS
browser-source integration/progress, not just the public video page or backend
WebSocket. Information/weather remain overlay-only; Lightning amounts show sats.
No Monero payment chooser or fiat account screen is needed to deliver that path.

## Technical references

- [Strike architecture](architecture/phase1-strike-architecture.md) and
  [address registry](architecture/lightning-address-registry.md).
- [Gateway boundary](security/openhab-feeder-gateway.md),
  [weather/overlay](architecture/weather-overlay.md),
  [overlay stream](architecture/overlay-stream.md) and
  [WireGuard topology](deployment/wireguard-topology.md).
- HOME: [gateway handoff](deployment/home-gateway-agent-handoff.md).
- VPS: [remediation handoff](deployment/new-vps-remediation-handoff.md).
- [Deployment artifacts](deployment/deployment-artifacts.md),
  [audit findings](deployment/audit-remediation.md),
  [threat model](security/phase1-threat-model.md),
  [hardening checklist](security/phase1-hardening-checklist.md) and
  [implementation history](implementation-status.md).

Apply the current roadmap before following older launch or immediate-XMR instructions
in these references. Reuse existing reviewed work. Do not ignore an actual defect,
waive a required code review, or report a mock as live evidence.

## Subsequent deliverables

[Square #104](https://github.com/lightning-goats/lightning-goats/issues/104) is next:
anonymous browser accounts buy integer Feedings and redeem them through the same
physical-owner safeguards. These are separate entitlements, not sats or a wallet
balance. Lightning stays account-free. The existing Square product/acceptance plan
is retained; no pricing or new fiat API is selected by this documentation.

[Multi-asset/Monero design](architecture/multi-asset-payments.md),
[XMR quote service](architecture/xmr-quote-service.md) and
[asset-credit storage](architecture/asset-credit-storage.md) remain preserved future
references. XMR integration/deployment is on explicit hold. Core shared BTC credit
and Lightning display work continue; #99's XMR portion must not block them.
[CyberHerd](architecture/cyberherd-phase2-boundary.md), new providers, broader site
redesign and optional tools are not first-deliverable dependencies.

## Addresses, ownership and tracking

Keep herd/dexter/rowan/cosmo/newton/nova configured with the common herd pool; the
pilot edge may initially expose herd only. Both old and new payment services must
respect one physical owner or only one dispatcher may be enabled. Keep separate
pilot/old ledgers and preserve real pilot credit at cutover. Never put the OpenHAB
token on the VPS. The old WireGuard hub stays `10.8.0.1` during the pilot.

#6 tracks migration; #15 observed acceptance; #16 cutover; #17 coordination;
#21 weather; #106 WAF; #104 next Square deliverable; #94 deferred Monero. Leave
unfinished acceptance and deferred roadmap issues open with truthful dispositions.
Earlier CLN/clnaddress setup documents are historical, not new-runtime instructions.
