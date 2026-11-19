# Delivery roadmap — Lightning first, Square second

**Operator decision: 2026-09-17.** The delivery order puts the core Lightning
payment and feeding flow first. Live activation follows the individual deployment
and cutover approvals.

| Order | Deliverable | Tracking and disposition |
| --- | --- | --- |
| 1 — current | Minimal Strike-backed Lightning Goats | #6, #15/#16, existing gateway/presentation work, and VPS WAF #106 |
| 2 — next | Square fiat purchases of anonymous stored Feedings | Existing approved product in #104; not a Lightning prerequisite |

CyberHerd, NIP-05 verification, additional payment providers, a general website
redesign and automated swaps/refunds are not added to either active deliverable.
Older documents naming CyberHerd as “Phase 2” describe a future architecture, not
the currently selected second payment deliverable.

## Cashu exception — operator direction, 2026-11-15

The [Nut November plan](cashu-nutnovember-implementation-plan.md) authorizes
ordinary Cashu and NIP-61 receiving through one isolated wallet and acquisition
engine, shared herd credit, constrained Cashu-to-Strike treasury settlement and
the specified two-QR overlay. This is the narrow exception to the alternative
provider and automated conversion deferrals above. Preserve existing CyberHerd
behavior and opt-outs, Lightning, Square entitlements and paid history.
Production keys, mints, limits, schema upgrades, public announcements,
financial activation and physical feeding remain operator-controlled.

## 1. Minimal Lightning deliverable

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

## 2. Square fiat Feedings

[#104](https://github.com/lightning-goats/lightning-goats/issues/104) remains the
product contract. Viewers buy integer **Feedings**, stored against an anonymous
browser account, and redeem them later. Square handles card details; the project
retains only the minimal identity, entitlement and reconciliation records needed.
Lightning remains account-free and follows its existing automatic sats-threshold flow.

**Square Feedings are separate entitlements.** Keep their balance separate from
the herd sats ledger and use the shared physical-owner safeguards. A fiat purchase itself does not
actuate the feeder: redemption durably reserves a Feeding and resolves the same
correlated physical request without double-spending or ambiguous fresh-ID retries.

Implement in bounded slices after the Lightning deliverable: anonymous identity
and entitlement storage; verified Square checkout/payment reconciliation and
idempotent grants; safe redemption; minimal browser controls; refund/dispute state,
edge/WAF rules and acceptance tests. Revalidate current Square APIs when coding.
Do not expand into Nostr login, broad PII collection or extra payment methods
merely because a future platform might support them. Retain #104's existing
acceptance criteria and operator decisions; this roadmap does not select pricing.

## Issue disposition and agent coordination

| Issue/scope | Current handling |
| --- | --- |
| #6, #15/#16, core gateway/Lightning presentation | Current Lightning implementation and observed acceptance |
| #106 | Current VPS WAF for Lightning; Square-specific additions in deliverable 2 |
| #104 | Next deliverable: Square stored Feedings and redemption |

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
