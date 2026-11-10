# Outgoing worker contract

This records the provider contract rechecked on 2026-11-10 while verifying
[frozen planning](cyberherd-reward-planning.md). It is not implemented spending
authority or an instruction to change production credentials. The remaining work
belongs to requirement 8 of the full port and must preserve the operator's
existing balance ceiling/sweep policy.

## Confirmed provider interface

For a fixed-amount BOLT11, request a quote with
`POST /v1/payment-quotes/lightning`, explicitly selecting BTC and omitting the
amount override. Persist a random UUIDv4 idempotency key first. Reusing it can
return the original quote ID in a duplicate response. Compare all returned BTC
amounts exactly; require net amount equality, bounded fee and total no greater
than the frozen gross. This endpoint needs
`partner.payment-quote.lightning.create`.
[Strike quote reference](https://docs.strike.me/api/create-lightning-payment-quote/)

Execution uses `PATCH /v1/payment-quotes/{id}/execute` and requires
`partner.payment-quote.execute`. Retain both IDs: execution returns a payment ID
distinct from the quote ID. HTTP 202 is accepted/pending, not delivery evidence.
The current state field can be pending, completed or failed; deprecated result
and delivered fields are not the state machine.
[Execution reference](https://docs.strike.me/api/execute-payment-quote/),
[sending walkthrough](https://docs.strike.me/walkthrough/sending-payments/)

Reconciliation reads `GET /v1/payments/{id}` with `partner.payment.read` and checks
identity, BTC amount/fee/total and current state before accepting finality.
Provider-to-provider optimization can omit Lightning-specific fields, so missing
network details alone do not prove failure. A preimage, if present, must match
the frozen invoice hash. Neither quote creation nor a relay receipt proves payment.
[Payment read reference](https://docs.strike.me/api/get-payment-by-id/),
[sending walkthrough](https://docs.strike.me/walkthrough/sending-payments/)

Error parsing uses the documented structured code/properties, never error-message
text. `DUPLICATE_PAYMENT_QUOTE` supplies a quote ID; `PAYMENT_PROCESSED` supplies
a payment ID. `LN_INVOICE_PROCESSED` alone cannot prove this worker paid this member.
Unknown/timeouts and malformed error responses retain uncertainty rather than
inventing a terminal outcome. Avoid logging provider messages/trace data or sending
them to the overlay/Nostr.
[Strike error reference](https://docs.strike.me/api/)

## Persist before each boundary

The [private payout authority](cyberherd-payout-authority.md) now implements
separate-role reservation/source readback and consumed-receive credential
qualification. The [outgoing invoice boundary](cyberherd-outgoing-invoices.md)
now persists callback/signature bytes and validated invoices. The
[provider execution journal](cyberherd-provider-execution.md) now saves quote and
execute intent, recovers the original payment ID and verifies payment reads. The
remote broker/runtime and authoritative terminal budget lifecycle remain unfinished.

The payout identity must first authenticate the core reservation independently;
copied membership DB rows/JSON are not financial authority. Pin its actual provider
account/environment and credential identity, with no public ingress or feeder
capability. Core retains no spend key. The existing receipt's legacy account label
is not sufficient account attestation; provider/account qualification remains open.
No additional account/profile scope should be guessed before checking its endpoint.

Persist the row's exact planned destination/amount/fee ceiling, callback terms,
invoice hash and unique source identity. When Nostr is supported, the signing
broker must freeze the exact 9734 bytes before the callback; never publish it to a
relay or downgrade on failure. Ordinary fallback remains explicitly recorded.
Revalidate public-network boundaries for each callback and verify mainnet BOLT11
signature, exact amount/description hash and expiry before requesting any quote.

Persist a quote-request intent before POST. A lost quote response cannot authorize
execution using only a duplicate quote ID: its fees/amount/expiry remain unverified.
No quote-read endpoint has yet been qualified. Retiring/replacing a quote is safe
only when the durable state and worker protocol prove no execute intent ever
existed. Qualify that recovery path with interrupted-request fixtures.

Persist execute intent before PATCH. From then on, uncertainty never receives a
new invoice, quote or payment identity. Retry/reconcile only the same quote;
correlated processed responses can recover the original payment ID for reads.
If the provider cannot establish the original outcome, hold for reconciliation.
Expired quotes/invoices do not erase an earlier possible execution.

Terminal outcomes must reach core through a narrowly authenticated payout role,
with authoritative verification before releasing unused reservations or aging
actual spending into the 24-hour window. Creation time cannot age out an unknown
payment. Reward announcements use only verified completed payments; monetary
recovery remains independent of waiting for a real member-thread publication
receipt. Skipped/unaffordable shares have no carry-forward member balance.

Required fixtures include lost quote/execute responses, interrupted commits,
same-quote recovery, wrong account/amount/hash/currency, expired/unaffordable
quotes, duplicate payment hashes, negative/non-sat-aligned fees, internal Strike
payments without a preimage, conflicting terminal states and restart/restore.
Mocks establish local protocol behavior; actual separate-role installation,
provider qualification and explicit financial activation remain separate evidence.
