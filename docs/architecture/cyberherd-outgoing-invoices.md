# Frozen outgoing invoices

This advances requirement 8 of the [full port](../planning/cyberherd-implementation-plan.md)
on top of the separate [payout journal](cyberherd-payout-journal.md). Invoice
validation is neither permission to execute a payment nor evidence of settlement.

## Persisted boundaries

An admitted payable row is required before the payout store creates an invoice
intent. It freezes member, owner, address, exact recipient metadata/callback,
whole-sat net amount, creation time and canonical root-configured receipt relays.
Concurrent/restarted preparation returns the same UUID and unsigned fields.

For a recipient advertising Nostr support, the prepared kind-9734 intent targets
the member's profile, with exact amount, encoded LNURL, receipt relays and receipt
signer. Zap signing uses the isolated remote broker. A successful response must have the
project signature and exactly the frozen kind/timestamp/content/tags. The store
commits the original signed JSON bytes before returning a callback capability.
Retries cannot replace those bytes, including whitespace changes that preserve the
Nostr ID but change the invoice description hash. Kind 9734 is not a relay post.
[NIP-57](https://github.com/nostr-protocol/nips/blob/master/57.md)

The 2026-11-15 operator decision prefers, but does not require, NIP-57 for
rewards. Missing/malformed zap capability or signing failure uses a durably frozen
ordinary callback. A valid zap request is still frozen exactly before callback
I/O. If its callback fails or returns an invalid invoice, one bounded ordinary
callback retry uses the same endpoint, amount and optional comment without Nostr
parameters. A signed invoice returned without zap binding can be paid as ordinary
Lightning; this never establishes a zap receipt or authorizes a second payout.

Actual NIP-57 requires the exact SHA256 of the frozen signed JSON. Ordinary
invoices may bind the raw LNURL metadata hash or carry a signed text description.
Text-description fallback is an explicitly authorized ordinary-payment compatibility
policy: it authenticates the callback through the frozen public HTTPS endpoint,
not through the LNURL metadata hash. Signature, network, exact amount, expiry,
payment-hash uniqueness, reservations and execution idempotency remain mandatory.
A different, unrecognized description hash still fails closed. Incoming invoice
validation is unchanged. The quote's NIP-57 bit records the validated invoice's
actual mode, not merely the wallet's advertised capability.

A receiver-advertised `commentAllowed` permits `CyberHerd Treats!`, clipped to its
limit and capped at 256 characters. This query comment is best-effort wallet
presentation, not a guarantee about its invoice description or displayed memo.
Old recipient JSON omits the new zero-default comment limit. No migration or
historical invoice, signature, quote, execution or outcome rewrite is permitted.

## Callback and invoice checks

The structured callback GET revalidates the recipient-supplied HTTPS origin and
resolves/pins only public IP answers. No environment proxy, redirect or success
action is followed. Signed JSON query bytes have their own bounded budget rather
than weakening the 2 KB limit for an untrusted supplied callback. Response size,
JSON content type and total/DNS/connect/read deadlines remain bounded. Callback
capabilities have no generic publication method or credential. Transport/provider
error bodies are not exposed in returned diagnostics.

The returned BOLT11 must have a valid mainnet signature, exact nonzero whole-sat
amount and applicable description commitment. Future timestamps and less than thirty
seconds of remaining validity fail. The validated invoice is durably frozen before
any future quote call. Its payment hash is unique across the payout database;
another grant cannot create a second payment for that hash. A later callback result
cannot replace a saved invoice. After saving, callback recovery is closed.

Historical saved validation remains readable after expiry for reconciliation.
It does not establish current validity: the future quote/execute protocol must
check current expiry and fresh financial policy again. Expiry never erases an
earlier possible payment. Grant capacity remains held until authoritative outcomes.

## Remaining work and verification boundary

The separate payout migrations add immutable intent/request/invoice records and an
append-only invoice journal. They do not modify the core or membership schemas.
Tests use synthetic signatures/invoices, temporary databases and loopback receive
qualification. They exercise ordinary and supported request freezing, exact-byte
restart recovery, zap-preferred ordinary fallback, altered callbacks/invoices, duplicate payment hashes,
expiry and protocol-query bounds. Their actual results must be recorded on the
checked source; adding fixtures does not itself claim they passed.

No production callback is invoked by these tests. Subsequent
[payout runtime](cyberherd-payout-runtime.md),
[core outcome verification](cyberherd-payout-outcomes.md) and
[member delivery](cyberherd-member-delivery.md) source supplies the broker,
provider recovery, frozen editable zap text and joining references. Reviewed
service installation and live qualification remain separate; none is activated
by an invoice-boundary fixture.
