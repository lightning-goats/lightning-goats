# Delivery roadmap — Lightning first, Square second

**Operator decision: 2026-09-17.** This delivery order supersedes the 2026-09-15
instruction to implement Monero immediately or finish two-rail presentation before
shipping Lightning. It does not change paid accounting, approve unreviewed code,
or authorize live host/payment/physical actions.

| Order | Deliverable | Tracking and disposition |
| --- | --- | --- |
| 1 — current | Minimal Strike-backed Lightning Goats | #6, #15/#16, existing gateway/presentation work, and VPS WAF #106 |
| 2 — next | Square fiat purchases of anonymous stored Feedings | Existing approved product in #104; not a Lightning prerequisite |
| Future — on hold | MoneroPay/XMR integration | #94 and XMR portions of #95–#100; resume only on explicit operator instruction |

CyberHerd, NIP-05 verification, additional payment providers, a general website
redesign and automated swaps/refunds are not added to either active deliverable.
Older documents naming CyberHerd as “Phase 2” describe a future architecture, not
the currently selected second payment deliverable.

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
  integration and reconnect behavior without waiting for XMR fields or a payment
  chooser. The public index page is not automatically the OBS browser source.
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

**Square Feedings are not sats, BTC or a deposit into the herd sats ledger.** Do not
reuse Monero FX quoting or manufacture BTC receipts to implement them. Share the
physical-owner safeguards, not the balances. A fiat purchase itself does not
actuate the feeder: redemption durably reserves a Feeding and resolves the same
correlated physical request without double-spending or ambiguous fresh-ID retries.

Implement in bounded slices after the Lightning deliverable: anonymous identity
and entitlement storage; verified Square checkout/payment reconciliation and
idempotent grants; safe redemption; minimal browser controls; refund/dispute state,
edge/WAF rules and acceptance tests. Revalidate current Square APIs when coding.
Do not expand into Nostr login, broad PII collection or extra payment methods
merely because a future platform might support them. Retain #104's existing
acceptance criteria and operator decisions; this roadmap does not select pricing.

The generic future native-plus-sats display rule does not alter Square's entitlement
accounting. Presentation must not imply an executed conversion or native BTC receipt.

## 3. Monero: preserve, do not continue

The existing code and design are valuable but **MoneroPay/XMR implementation,
new integrations, deployment, wallet provisioning, oracle activation and XMR UI
are on hold until the operator explicitly resumes this work**. Completion of
Lightning or Square, available agent tokens, a green CI badge or a timer tick
is not implicit permission to resume.

The current narrow exception is publishing the already prepared #97 candidate and
merging only if appropriate. It is preserved in
[PR #107](https://github.com/lightning-goats/lightning-goats/pull/107), branch
`roadmap/monero-bridge-97-preserved`, source
`c6e5efce635f11169e3c7381ae28a8b4cd0e9568`. Keep it draft/unmerged pending its
independent security/correctness review; source preservation is not a reason to
add an unreviewed binary/build dependency to the first release. If later reviewed
and merged under operator authority, that still does not authorize XMR activation.

Preserve the merged #102/#103/#105 domain/storage/quote work, tests, migration
history and any real paid records. Do not roll back databases, rewrite signed
messages, or remove generic accounting just to pause an unused rail. Keep XMR
endpoints/controls/services absent or disabled. Existing tests may continue in
normal shared CI; that is not a resumed Monero feature assignment.

Retain [the multi-asset design](../architecture/multi-asset-payments.md) and
[#94](https://github.com/lightning-goats/lightning-goats/issues/94) as the future
roadmap. The deferred messaging decision remains BTC/Lightning sats-only and XMR
native-plus-credited-sats, without exposing transaction/address/wallet/capability
identifiers. No new XMR event/renderer implementation is requested now.

## Issue disposition and agent coordination

| Issue/scope | Current handling |
| --- | --- |
| #6, #15/#16, core gateway/Lightning presentation | Current Lightning implementation and observed acceptance |
| #106 | Current VPS WAF for Lightning; Square-specific additions in deliverable 2 |
| #104 | Next deliverable: Square stored Feedings and redemption |
| #94, #96, #97, #98, #100 | Future Monero roadmap on hold; preserve evidence and unresolved findings |
| #95 | Keep shared BTC accounting valid; defer XMR-only follow-up |
| #99 | Defer XMR choice/dual-amount presentation; do not defer required Lightning OBS work |

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
