# Multi-asset payments and sats-denominated feed credit

Operator-approved direction: **2026-09-15**. Tracker: [#94](https://github.com/lightning-goats/lightning-goats/issues/94), under [#6](https://github.com/lightning-goats/lightning-goats/issues/6).

This adds MoneroPay to Strike, not a replacement Lightning provider. It supersedes
older **Strike-only / no new backend** scope statements only for this workstream.
The [parallel live pilot](../deployment/parallel-live-pilot.md) remains independently
usable with Strike. Unimplemented Monero work is not a blanket pilot blocker.
This document authorizes repository development, not live wallet provisioning,
network changes, payments or physical feeding. CyberHerd stays a later phase.

## Decision and invariants

The canonical display and feeder quantity is **project feed credit in sats**:

```text
Strike BTC receipt ----- identity valuation --+
                                              +--> feed-credit grant --> herd ledger
MoneroPay XMR receipt --- locked quote --------+                          |
                                                    +--------------------+
                                                    v
                                          existing feeder and overlay
```

This is consumption accounting, not a redeemable wallet balance. XMR stays XMR;
valuing it in sats does not execute a trade or promise BTC redemption. Treasury
balances/sweeps and subsequent FX changes must not change previously granted credit.
A confirmed feed consumes the configured threshold; pending/ambiguous actuation
never consumes credit or authorizes a fresh physical retry.

Keep separate records for:

| Record | Authoritative content |
| --- | --- |
| Payment intent | Project/goat/pool, asset/network, quote and unique receive binding |
| Asset receipt | Actual asset/atomic amount, provider identity, scoped transaction evidence and finality |
| Quote/valuation | Immutable agreed atomic-to-credit ratio, rate direction/source/time and policy version |
| Credit allocation | Cumulative eligible atomic amount, cumulative credit, receipt allocation and new delta |
| Existing herd ledger | Positive granted sats and negative confirmed-feed debits |

BTC atomic units are satoshis; XMR atomic units are piconero. No XMR value may be
put into the old `SettledPayment.amount_msat` field as if actual BTC arrived.
Preserve the BTC/Strike adapter and signed invoice checks during additive migration.
Backfill evidence/valuation references without inserting a second historical credit.

## Component boundaries

- `lightning-goatsd`: payment intents, asset adapters, quotes, authoritative reads,
  durable credit/accounting and events. It does not scan Monero blocks/wallet RPC.
- Existing home OpenHAB gateway: unchanged physical authority and weather boundary.
- **Separate home Monero bridge**: project-bound receive creation/status, local
  MoneroPay notification processing and durable first-seen observations. No
  OpenHAB token, physical route, transfer route or generic reverse proxy.
- MoneroPay and wallet RPC: loopback/private host services behind that bridge.
  Prefer a dedicated view-only project wallet after validating the pinned stack;
  keep spend key off the service host. View keys/addresses/history remain private.
- Rate provider: replaceable read-only oracle interface. No exchange trading key.

MoneroPay documents `POST /receive`, `GET /receive/:address`, optional callbacks,
partial coverage and unlocked coverage. It also exposes `/transfer`, which is
specifically outside the bridge's authority. Its documentation encourages higher-
level reconciliation when callbacks are lost. Verify the selected release's actual
identity/aggregation and view-only behavior before integration acceptance. [1][2][3]

**Port collision:** this home host already uses port 5000 for the weather receiver.
MoneroPay's documented default is also localhost:5000. Inventory/select a different
explicit loopback port; do not replace/rebind/restart weather to make room. New
bridge/MoneroPay/wallet ports are deployment values, not assumed reservations. [4]

## Receive and recovery flows

### Lightning (unchanged externally)

Native address/LNURL -> Strike signed BOLT11 -> authenticated notification/inbox ->
authoritative Strike receive -> BTC identity valuation -> atomic credit/event.
Creation of an invoice is not payment. Monero/rate downtime cannot stop this rail.

### Sats-targeted Monero quote

1. Viewer selects goat and target feed-credit sats, then Monero.
2. Fetch a fresh rate with explicit direction **sats per XMR**. Pin source/time,
   rational numerator/denominator, limits and expiry; no floating-point money.
3. Calculate requested piconero by rounding **up**. Persist immutable quote and
   project intent, then obtain a unique subaddress through the home bridge.
4. Expose the payable address/URI only after the durable binding is complete.
   A repeated creation ID returns the same binding. A lost upstream creation reply
   is recorded as ambiguous, not a reason to expose multiple payable addresses.
5. MoneroPay reports detection/unlock. Its local callback schedules a trusted GET;
   neither a callback amount nor an unauthenticated timestamp grants credit.
6. Persist normalized receipt evidence and first-seen time. Pending XMR can be
   shown as pending; only **unlocked, non-double-spend, eligible** receipts qualify.
7. Commit new receipt allocation, cumulative watermark, credit delta and public
   event in one serialized transaction. Existing feed logic consumes that credit.

Callbacks are latency hints. A bounded durable inbox and startup/periodic recovery
must also reconcile outstanding, recently paid and expired-but-still-payable intents.
Never sum callbacks. Do not read wallet balance as received revenue. Optional
home-to-VPS notifications must be authenticated, but authoritative per-intent reads
still decide credit. No callback signature is assumed absent pinned-source proof.

MoneroPay's documented callback and GET shapes differ (`transaction` versus
`transactions`). GET can be height-filtered: filtered/partial history cannot be
mistaken for a complete cumulative total. The `complete` flag alone is insufficient
for validation. Inspect transaction identity, amount, lock/double-spend state and
aggregate consistency. Key identity by asset/network/provider plus project receive
binding and the provider's actual transaction/output aggregation semantics. The same
transaction may legitimately pay multiple subaddresses; tx hash alone is not a
project-wide unique receipt. [1][2]

## Immutable valuation and edge cases

For quote target `S`, agreed requested piconero `A`, and cumulative eligible
piconero `U`:

```text
required_atomic = ceil(target_sats * 10^12 * rate_denominator / rate_numerator)
total_credit    = floor(U * S / A)
new_credit      = total_credit - already_credited_sats
```

Use checked integer arithmetic and explicit storage/operational bounds. The
agreed `S/A` ratio, not a fresh oracle price, values partials and timely overpayments.
An exact full payment earns exactly `S`. Persist zero-credit dust receipts and
atomic watermarks too; otherwise fragment rounding can lose value or replay dust.
Validate that prior credited sats equal the valuation of prior eligible atomic
amounts. Regressed totals/corrupt prior allocation require reconciliation, not
saturating subtraction, silent repair or lowering a watermark then re-crediting it.

| Situation | Initial behavior |
| --- | --- |
| Zero/one confirmation, still locked | Pending only; no available feed credit |
| First trusted durable observation within quote window, later unlock | Original quote honored; confirmation delay does not expire it |
| Receipt exactly at/after expiry or unknown first-seen evidence | Preserve funds/evidence; hold for explicit valuation, do not reuse expired quote |
| Partial then late top-up | Evaluate each receipt's timing; first receipt does not indefinitely extend the quote |
| Timely overpayment | Same immutable rational ratio within configured bounds; out-of-bound amount is held, not discarded |
| Rate outage/staleness | Refuse new XMR quote; honor valid existing quotes; native BTC continues |
| Duplicate/reordered notification | Re-read known intent; no extra receipt/grant |
| Reorg, double-spend or authoritative evidence regression | Persistent hold for affected intent; no automatic fresh credit or reversal of consumed feeds |
| Wallet swept | Historical receipts and feed credit unchanged |
| Late/unquoted payment | Visible private reconciliation case; no automatic spending/refund |

Quote window is `[issued_at, expires_at)`. First-seen time must be recorded by the
trusted bridge while reading authoritative state, not taken from the client, block
timestamp, public callback, or time of later polling after an outage. This is a
conservative merchant policy, not a claim to know when the sender broadcast.
If timing cannot be established, retain an explicit hold. Exact quote lifetime,
rate-age limit, donation limits and live oracle choice are deployment policy inputs.
Fixed-address arbitrary donations/automatic refund/trading remain out of scope.

## Overlay, messaging and privacy

Keep `feed_credit_sats`, `threshold_sats`, `feeds_due` and `remainder_sats` stable.
The client uses absolute sequenced ledger snapshots/events, not wallet balance,
Nostr counts or locally accumulated callback amounts. Pending XMR is separate from
funded credit. Funding and feeder availability/confirmation are separate displays.
The suggested bar stays full while a feed is funded and drops after confirmed
consumption; the exact visual choice is not finalized by this document. A cumulative
per-stream goal is a different metric and must not be inferred from unspent credit.

Project receipt models are **private**. Build explicit public projections before
feeding `/ws/overlay` or Nostr: no XMR tx hash, subaddress, wallet/provider receipt
ID, view/spend key, callback capability, status capability or quote internals.
**Amount display approved by the operator, 2026-09-15:** BTC/Lightning payment
messages show credited sats only. Monero messages show actual XMR received plus
credited sats. Other future non-BTC payment assets show their native amount/unit
plus credited sats. Apply this to BOTH Nostr and the video overlay. This supersedes
the earlier default hiding native asset/amount, not the private-field exclusions.

Native amounts come from verified receipts/allocations, not reverse conversion of
rounded sats or a new market price. Keep exact atomic precision and deterministic
decimal formatting; describe sats as credited value, not an executed trade. For
partials/dust distinguish the latest receipt from cumulative credit/rounding carry.
Do not make a duplicate replay look like another payment or rewrite old signed
messages. Amount/timing disclosure can correlate donations; do not promise anonymity.
Renderer/public-event wiring remains [#99](https://github.com/lightning-goats/lightning-goats/issues/99).
The #103 storage projection is still sats-only until that wiring is delivered.
Preserve deterministic templates, exact signed-event retries, and overlay-only
information/weather.

The OBS browser-source integration remains distinct from the public site. Introduce
the two-rail payment choice/progress/status contract without a general website rewrite.
XMR controls/routes are disabled until implemented and reviewed; no dead payment UI.

See [the quote-service implementation and activation boundary](xmr-quote-service.md)
for #96. Creating quote terms alone does not create an address or grant credit.

## Work order and ownership

| Issue | Deliverable | Sequence |
| --- | --- | --- |
| [#95](https://github.com/lightning-goats/lightning-goats/issues/95) | Amount/valuation domain; durable receipts/grants and BTC compatibility | First |
| [#96](https://github.com/lightning-goats/lightning-goats/issues/96) | Fresh oracle and immutable quote lifecycle | Domain first; then alongside storage |
| [#97](https://github.com/lightning-goats/lightning-goats/issues/97) | Separate receive-only home bridge and inactive scripts | Parallel with ledger/quote work |
| [#98](https://github.com/lightning-goats/lightning-goats/issues/98) | Intents, notification inbox and independent recovery | After ledger/quote/bridge contract |
| [#99](https://github.com/lightning-goats/lightning-goats/issues/99) | Payment chooser, actual overlay client and safe projections | After credit/API contract |
| [#100](https://github.com/lightning-goats/lightning-goats/issues/100) | Mixed-rail tests, coordinated restore and deploy handoff | Incrementally; before XMR activation |

Lead handles bounded repository engineering with actual available tools. HOME/VPS
retain host-specific responsibilities when available; no new Codex work is assumed.
Use #17 for claims and #94 for this workstream. No second owner or coordination
framework is needed. Existing pilot/human review decisions remain intact.

First source slice: `src/domain/credit.rs` and `tests/credit_valuation.rs` provide
pure amount/rate/quote/cumulative valuation primitives, not a running payment rail.
They deliberately do not serialize private payment evidence, perform network I/O,
change migrations, or grant ledger credit. Durable intent binding, quote persistence,
serialized writes, eligibility and recovery remain in #95/#96/#98. Keep those issues
open until implemented; do not count arithmetic tests as integrated payment tests.

## Rollout and verification

Use additive storage migration and retain the existing BTC ingress while it moves
onto the new credit primitive. Snapshot/back up populated state; migration must
preserve every actual credit, issued request, pending feed UUID and signed event.
Rehearse restore across quote/receipt/watermark/ledger and home receive/provider
mapping/scan state. A restored receipt database alone is not evidence that previously
funded physical actions did not occur. Reconcile first and rotate overlay identity
on offline restore. Never reset paid state to retry deployment.

Tests include mixed rails totaling 2340 synthetic credit sats -> two confirmed
harmless feeds -> 340 remainder; dust/splits/replays/concurrent workers; late/unlock
and rate boundaries; create-response loss; double-spend/regression; crashes around
credit/event commit; privacy projection; wallet sweep; and restore. Use the real
Rust services against local mocks where relevant. Selected MoneroPay/wallet-RPC
behavior and operator-observed XMR settlement are separate deployment acceptance.
No new public endpoints, real oracle calls, wallet keys or live hosts are touched
by the first pure-domain slice. Script preparation and inactive installation before
operator-run activation; only the XMR feature is held when its own path is incomplete.

## Primary upstream references (checked 2026-09-15)

1. [MoneroPay receive API](https://moneropay.eu/api/receive.html): creation, GET, covered/expected/unlocked and filtering.
2. [MoneroPay callbacks](https://moneropay.eu/api/callback.html): notification payload and recovery-read recommendation.
3. [MoneroPay endpoints](https://moneropay.eu/api/endpoint.html): piconero units and transfer surface.
4. [MoneroPay options](https://moneropay.eu/options/options.html): bind/RPC/polling configuration.
5. [MoneroPay zero-conf option](https://moneropay.eu/options/zeroconf.html): intermediate notifications are not our credit policy.

Docs specify contracts, not pinned deployment conformance. Record the actual
MoneroPay/wallet versions and provenance in the eventual private host preflight.
