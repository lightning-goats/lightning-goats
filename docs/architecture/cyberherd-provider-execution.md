# Durable outgoing Strike execution

This implements the provider/journal portion of requirement 8 in the
[full CyberHerd port](../planning/cyberherd-implementation-plan.md). It builds on
[qualified source funding](cyberherd-payout-authority.md) and
[frozen outgoing invoices](cyberherd-outgoing-invoices.md). It does not install or
enable a payout worker, release core reservations, or announce a member treat.

## Scope and provider boundary

The outgoing client fixes the production API origin and checks the exact root
credential fingerprint/account label against each committed request. Its key is
zeroized; proxies and redirects are disabled, requests and bodies have deadlines
and size limits, and returned errors omit raw provider bodies, messages and traces.
The label is not an API self-identity attestation. Actual consumed-receive access
with this same credential is qualified before admission into the separate store.
Test-only constructors require literal loopback and are absent from normal builds.

Opaque quote, execute and bound-payment capabilities come from the private journal;
deserializing provider JSON cannot create them. All provider calls remain outside
the receive daemon's configuration and credential domain.

## Quotes and safe retirement

The journal saves a UUIDv4 quote intent before POST. It sends the saved fixed
BOLT11 and explicitly selects BTC, without an amount override. The provider quote
must match exact whole-sat net amount and total, with nonnegative fees bounded by
the frozen fee/gross ceiling. Optional fee fields must agree with the exact
`totalAmount - amount` difference; missing optional fields do not invent zero fees.
Only BTC-to-BTC unit conversion is accepted. No FX or on-chain method is introduced.
[Quote reference](https://docs.strike.me/api/create-lightning-payment-quote/)

The provider expiry, when supplied, is retained. Independently, the application
limits a quote to thirty seconds from its saved request and never beyond invoice
expiry. Missing optional `validUntil` is recorded as missing, not a claimed
provider expiration; the local limit still applies. This preserves documented
same-currency/internal-payment responses without accepting indefinite validity.

A structured duplicate response saves only its original quote ID. It cannot
supply missing fees or authorize execution. No quote-GET endpoint is assumed.
Before any execute intent exists for that invoice, explicit transactional retirement
can replace a lost/expired quote with a fresh UUID for the same frozen invoice.
Retirement and execution serialize under one write lock. After an execute intent
exists, database guards and application checks forbid retirement or a fresh quote.
An expired saved invoice is held rather than silently replaced or represented as
a paid reward; no-execute terminal disposition belongs to the outcome protocol.

## Persisted execution and recovery

Execution requires both the independent root payout flag (default false) and a
fresh kernel-authenticated core funding page for the exact saved reservation,
owner, epoch, funding and core UID. Current percentage/per-feed/outstanding/fee
ceilings are checked again. The permit lasts at most five seconds. Supported
Nostr payouts also require the core's public-Nostr flag: the recipient may publish
9735 after receiving payment, even though this worker never publishes 9734.
Canary mode therefore cannot silently turn such a payout into an ordinary payment.

The execute intent commits before PATCH. The client uses only the saved provider
quote ID. A lost response leaves `ExecutionUnknown`; it never authorizes a fresh
invoice, quote or payment identity. A permitted recovery PATCH targets the same
quote, including after its nominal expiry, to recover the original payment ID.
A matching structured `PAYMENT_PROCESSED` response can supply that ID. Once bound,
recovery uses readonly GET rather than another execute request. Binding is global
and immutable; different IDs for one quote create a retained conflict.
[Execute reference](https://docs.strike.me/api/execute-payment-quote/)

HTTP errors are parsed by exact status/code/property, never by message text.
`LN_INVOICE_PROCESSED` alone supplies no proof that this worker paid the member.
Other errors, timeouts and malformed replies retain the original uncertainty.
[Error reference](https://docs.strike.me/api/)

## Authoritative observation, not a public food/reward claim

GET uses the payment ID obtained from that exact execute request or its correlated
processed response. Amount, currency, fees, total, method and optional preimage are
verified against the frozen quote/invoice. Documented Strike-internal payments may
omit Lightning details and preimage; this omission alone is not failure. Any
provided preimage must match. Completion timestamps are optional; supplied values
cannot precede the request or lie arbitrarily in the future.
[Payment reference](https://docs.strike.me/api/get-payment-by-id/),
[internal payment behavior](https://docs.strike.me/walkthrough/sending-payments/)

Pending/terminal observations are append-only and identical observations do not
create another transition. Contradictory terminal states/costs persist a conflict
and hold recovery rather than replacing the earlier evidence. Readonly recovery
continues while execution is disabled. Grants stay held until final core ACK under
the [verified outcome protocol](cyberherd-payout-outcomes.md), which independently
checks provider state/costs before releasing unused allocation. Creation time never
ages out unknown spending. A failed state never claims net delivery or a refund;
its full gross remains held for explicit cost reconciliation.

The fixtures exercise actual loopback HTTP/private Unix framing and temporary
stores: lost usable quote/execute results, same-identity recovery, id-only duplicate
refusal, no-execute retirement, root/core/public-receipt holds, policy changes,
wrong amounts/currencies/fees/expiry, internal transfers and terminal contradictions.
Actual check results must be recorded on the source pin. They do not qualify a
production account or authorize a financial activation.

The isolated payout runtime, signing broker and completed-payout/member-thread
announcements remain required for the full port. Core outcome source is not live
payout-worker acceptance or permission to publish a treat notice.


## Operator-selected inclusive fees — implementation in progress

The operator selected fees deducted from members' gross shares, supplied by the
Strike API. The 100-sat feeding and 1,000-sat rolling-day spending limits include
fees; the maximum delivered payment is 100 sats. There is no independent fixed
fee cap. A share of 100 sats with a quoted fee of 2 sats permits 98 sats delivered,
with 100 sats spent. Fees consuming the share or leaving less than the recipient's
minimum make that share unaffordable. Unused amounts remain with operations.

[Strike's quote API](https://docs.strike.me/api/create-lightning-payment-quote/)
returns receiver amount, total fee and total amount. Its `amount` override and
`INCLUSIVE` fee policy are documented for zero-amount invoices only. Existing
LNURL/NIP-57 invoices bind an exact amount and description hash, so adjusting a
payout requires a separately frozen invoice and, where supported, signed zap
request. Never change signed amounts or silently drop NIP-57 support.

The source includes bounded fee arithmetic, typed non-executable observations,
and append-only discovery records. Migration 0010 adds a separate chain for
pre-execution probe invoices. Each next amount must be strictly smaller and
follow the previous oversized provider quote; the original grant remains fixed.
A fitting quote ends adjustment. Discovery records alone cannot execute a payment.
The single executable invoice identity per grant/member remains unchanged.

The probe persistence adapter now freezes and restores each exact invoice intent,
callback/zap, validated BOLT11 and provider request. Historical reads reverify the
saved invoice against its original signed request. Observation loading can read
these separate identities; executable quote loading still requires the existing
execution-chain tables. A native restart fixture exercises a two-round reduction
and refusal to execute a fitting probe without explicit promotion; its new source
requires CI qualification.

New reward plans also retain the chosen custom zap template and frozen member
name, allowing each adjusted invoice to render its own exact net amount. Existing
plans without this optional field retain their original serialization and hashes;
a legacy rendered message cannot be reused for a different amount. Later template
edits do not change a frozen request. This is presentation evidence, never payment
authority.

The source now adds the explicit `deduct_from_share` mode to frozen planning
and the independent payout policy. Historical `fixed_allowance` is the default
and is omitted on serialization, retaining original plan hashes. Inclusive mode
requires zero fixed basis points/minimum fee and zero independent fixed fee cap;
the original gross allocation and feeding/day limits still bound all spending.
Modes must agree before new execution. Known-payment reads remain available after
a mode or limit change.

The worker freezes each round before signing/callback/provider work. A fitting
observation is atomically promoted using the same final invoice ID, exact zap and
BOLT11 bytes, provider request ID and provider quote ID. A separate immutable
promotion receipt is required for inclusive execution. Interrupted promotion rolls
back the entire execution chain. Only the existing core execution claim and
provider protocol can subsequently authorize spending. A lost execution response
recovers the original quote/payment identity. An expired unexecuted final quote
closes unused rather than creating a different invoice. An id-only duplicate fee
quote supplies no price evidence: retain its original ID and close the unused
share without claiming a payout.

Verified result intake accepts the actual reduced net plus fee only within the
original member gross and frozen recipient/planning minima. Historical fixed
plans retain their exact net/fee checks. The new worker, atomicity and result
fixtures require full CI qualification. Root financial-candidate rendering and
its independent deployment checks still require the explicit inclusive mode.
None of these source changes has been installed on the live payout journal.
Outgoing roles remain disabled; no partial configuration or activation is implied.
