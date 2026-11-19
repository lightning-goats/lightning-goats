# Delivery roadmap — Lightning and Cashu

**Operator decision, 2026-11-19.** Strike-backed Lightning and Cashu remain
live. Preserve paid history, the shared feeder controls and the
[architecture for future payment sources](../architecture/phase1-strike-architecture.md#future-payment-sources).
Any future source needs its own verified receipt and credit policy.

## Cashu — operator direction, 2026-11-15

The [Nut November plan](cashu-nutnovember-implementation-plan.md) covers
ordinary Cashu and NIP-61 receiving through an isolated wallet and acquisition
engine, shared herd credit, constrained Cashu-to-Strike treasury settlement and
the two-QR overlay. Preserve existing CyberHerd behavior and opt-outs. Production
keys, mints, limits, schema upgrades, public announcements, financial activation
and physical feeding remain operator-controlled.

## Lightning deliverable and acceptance

Use the existing Strike backend and current daemon/gateway architecture, not a
replacement wallet or a new framework. Start with the
[parallel live pilot](../deployment/parallel-live-pilot.md), preserving the old
system while the operator observes the new one. Minimal means a complete usable
Lightning path, not merely a successful invoice API call.

The deliverable includes:

- Lightning Address/LNURL-pay, authoritative Strike settlement and missed-event
  recovery; project feed credit denominated in sats, independent of wallet balance.
- One existing home physical owner through the narrow gateway, durable request IDs,
  override/enable/interval/cap controls, and debit only on confirmed feeding.
- Payment/goat-fact and confirmed-feed messages to Nostr and the actual OBS
  overlay. Information/weather remain overlay-only. Finish the needed browser-source
  integration and reconnect behavior. Verify the OBS browser source separately
  from the public index page.
- Usable public site/video/Lightning controls, with retired LNbits/NIP-05 routes
  removed or denied. Preserve configured herd/dexter/rowan/cosmo/newton/nova users;
  pilot exposure can initially be only herd at feeder.lightning-goats.com.
- VPS nginx/TLS and required ModSecurity v3/connector/CRS work in #106. Prove
  legitimate LNURL, exact-body signed Strike webhooks, status, overlay WebSocket,
  static assets and certificate renewal work through the selected WAF. Do not
  bypass all payment inspection or call detection-only tuning blocking acceptance.
- Pinned artifacts and repeatable configuration, inactive installation, preflight,
  backup and rollback steps. Keep real credentials/host values private and preserve
  actual paid state. Installation does not silently activate services or cut over DNS.

The operator's practical pilot policy remains in effect: no new blanket requirement
for every historical issue to close, every unrelated lab to run, or a Strike sandbox
before manual pilot payments. A concrete unsafe defect blocks its affected path.
The newly required WAF is part of the first public deliverable, not grounds to
interrupt an existing pilot without a scoped operator decision.

For completion, record the selected source/config, actual Lightning receipt and
single credit, observed correlated feeding and single debit, restart/replay behavior,
Nostr/OBS presentation and applicable edge controls. Do not label source-only tests
or static website staging as live payment/feeder acceptance. The operator selects
manual amounts/duration and the eventual public DNS/profile cutover.

## Issue disposition and agent coordination

| Issue/scope | Current handling |
| --- | --- |
| #6, #15/#16, core gateway/Lightning presentation | Current Lightning implementation and observed acceptance |
| #106 | Completed VPS WAF acceptance for Lightning |

Use #17 for concise source-pinned checkpoints. Lead prioritizes repository fixes,
release preparation and reviews needed for Lightning. HOME retains the OpenHAB
owner/gateway/weather/containment boundary; VPS retains Strike, nginx/WAF, Nostr,
site/overlay and VPS deployment. Coordinate shared-file changes; preserve other
agents' branches and unpublished work. Do not assume unavailable Codex capacity or
make a worker ACK a prerequisite for recording this direct operator scope decision.

“Deferred” is not “completed.” Keep unfinished roadmap issues open with a visible
hold, and do not auto-close parent acceptance trackers because source was published.
No private record, comment, merge or CI result grants new live privileges. Continue
independent safe Lightning work when a host-specific action needs the operator.
