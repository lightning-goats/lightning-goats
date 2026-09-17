# Lightning Goats documentation

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
