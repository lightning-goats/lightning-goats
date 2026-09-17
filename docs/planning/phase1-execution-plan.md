# Phase 1 execution — minimal Lightning payments

**Current operator decision: 2026-09-17.**
[Delivery roadmap](delivery-roadmap.md): Lightning first, Square #104 second,
Monero on hold until explicitly resumed. This replaces the previous immediate
multi-asset work order; no XMR integration or dual-asset UI is a Lightning blocker.
Preserve the merged foundations, tests and paid state rather than reverting them.

## Goal and operating policy

Deliver the existing Strike-backed daemon on the new VPS, the narrow home gateway,
real feeding through the existing owner, payment/feed messages and OBS progress,
and the usable public Lightning interface with required VPS WAF #106.
Use [the parallel live pilot](../deployment/parallel-live-pilot.md) at
`herd@feeder.lightning-goats.com`, keeping old production recoverable. The operator
selects manual payment amounts/duration and later public DNS/profile cutover.

The earlier practical pilot decision remains: no blanket HOLD, compulsory Strike
sandbox, every historical issue closed, or unrelated exhaustive laboratory work
before a first manual pilot payment. A real defect still blocks its unsafe feature.
There is no adopted two-payment/220-sat/30-minute budget. Read current evidence;
do not turn a historical mock result or unrun check into production acceptance.

## Ordered work

| Step | Deliverable | Evidence |
| --- | --- | --- |
| 1 | Select reviewed Lightning source and finish concrete defects in that path | Exact source, relevant tests/review, no duplicate integration branch |
| 2 | Prepare scriptable VPS/HOME config, TLS, isolated durable state, receive-only credentials and gateway binding | Pinned artifacts, private host values, inactive install/preflight and rollback |
| 3 | Configure the required nginx/ModSecurity/CRS boundary (#106) for existing Lightning routes | Legitimate LNURL/webhook/status/WebSocket/static traffic passes; attack fixtures blocked; private logs checked |
| 4 | Observe manual Strike payments and local feeding with operator | One verified receipt/credit, correlated completed feed/debit, no competing legacy dispatcher |
| 5 | Finish/verify Nostr and actual OBS browser-source behavior | Sats-only goat-fact/payment and feed messages, durable absolute progress, reconnect/restart; info/weather overlay-only |
| 6 | Hand off the minimal complete release and cut over when operator chooses | Short tested commands/config inputs, actual observations, preserved paid state and recoverable old system |

Steps 2/3 and browser-source work may run in parallel where file/host ownership
is clear. WAF tuning is not a reason to expose speculative Square/XMR routes, turn
off all payment inspection, or interrupt an existing pilot without a scoped decision.
Its blocking-mode acceptance is required for calling the new public deliverable
complete; detection-only mode is preparation, not equivalent protection.

## Practical acceptance boundaries

Before taking money, verify TLS/routing, signed invoice/authoritative settlement,
protected receive-only credentials, correct durable state, relevant tested source,
and a practical stop path. Do not advertise feeding enabled when it is unavailable.
Before feeding, verify correlated completion, durable duplicate handling, local
safety controls and arbitration with the old dispatcher. Only one existing owner
may actuate. An unknown outcome blocks a fresh retry, not the rest of safe work.

Keep the six configured goat users and common herd pool. Pilot edge can initially
expose herd only; roll out the other existing addresses without a registry rewrite.
The bar follows unconsumed project sats credit, never Strike balance or message
counts. A 2,340-sat synthetic example with two 1,000-sat confirmed debits leaves
340; it is a regression target, not a required real purchase or pilot budget.

The public site is not the OBS rendering page. Core payment/feed display and the
actual browser source belong to this deliverable; larger redesign, XMR fields,
fiat account UI and CyberHerd do not. Preserve existing weather/info behavior;
weather polish does not justify fabricating freshness or blocking unrelated tests.

## Next deliverable and deferred work

After minimal Lightning, implement Square #104's anonymous stored integer Feedings
and safe later redemption, separate from the sats ledger. Do not route fiat through
XMR quoting, require Lightning accounts, or create a second feeder owner.

Monero parent #94 and XMR-specific work in #95–#100 remain on hold. The exception
requested now is preserving the prepared #97 work in draft PR #107 and merging
only when appropriate under existing review requirements. No continued #98 coding,
MoneroPay installation, new Monero tests/feature expansion or wallet/rate activation
is authorized by the old plan. Ordinary shared CI may keep testing existing code.
Core Lightning overlay work is not paused with #99's XMR-specific extension.

## Responsibilities, verification and rollback

HOME retains physical owner/gateway/OpenHAB/weather/home containment; VPS retains
Strike/nginx/TLS/WAF/Nostr/site/overlay/VPS configuration. Lead handles bounded repo
work/review/integration. Use #17 for short exact-source handoffs, preserve branches,
and do not assume available Codex tokens or invent private-record authority.

Use [the verification matrix](../testing/phase1-verification-matrix.md), applicable
checks and existing exact-candidate evidence. Preserve independent review of
safety-critical source and protections; this operator-directed planning edit does
not approve unreviewed runtime changes or perform host operations.

Stop only the affected new ingress/dispatcher on failure. Keep real receipts,
issued requests, credit, pending UUIDs and signed events. Resolve ambiguous physical
outcomes before changing dispatchers. Never reset paid state or restore an older
ledger as an easy rollback. See
[cutover and rollback](../deployment/production-cutover.md). Scripts must leave
installation, activation and public cutover explicit, not hidden side effects.
