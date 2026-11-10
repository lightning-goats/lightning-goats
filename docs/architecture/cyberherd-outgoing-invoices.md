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
signer. The remote signing broker remains required. Its response must have the
project signature and exactly the frozen kind/timestamp/content/tags. The store
commits the original signed JSON bytes before returning a callback capability.
Retries cannot replace those bytes, including whitespace changes that preserve the
Nostr ID but change the invoice description hash. Kind 9734 is not a relay post.
[NIP-57](https://github.com/nostr-protocol/nips/blob/master/57.md)

Unsupported recipients record explicit ordinary-Lightning fallback. Supported
recipients cannot fall back because signing or invoice retrieval failed. Ordinary
invoices commit to the raw LNURL metadata hash; Nostr invoices commit only to the
SHA256 of the exact frozen signed request JSON.
[LUD-06](https://github.com/lnurl/luds/blob/luds/06.md)

## Callback and invoice checks

The structured callback GET revalidates the recipient-supplied HTTPS origin and
resolves/pins only public IP answers. No environment proxy, redirect or success
action is followed. Signed JSON query bytes have their own bounded budget rather
than weakening the 2 KB limit for an untrusted supplied callback. Response size,
JSON content type and total/DNS/connect/read deadlines remain bounded. Callback
capabilities have no generic publication method or credential. Transport/provider
error bodies are not exposed in returned diagnostics.

The returned BOLT11 must have a valid mainnet signature, exact nonzero whole-sat
amount and expected description hash. Future timestamps and less than thirty
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
restart recovery, no downgrade, altered callbacks/invoices, duplicate payment hashes,
expiry and protocol-query bounds. Their actual results must be recorded on the
checked source; adding fixtures does not itself claim they passed.

No production callback is invoked by these tests. The signing broker, provider
quote/execute/reconciliation state machine, terminal-outcome accounting and isolated
worker/service installation are unfinished. None is activated by this source.
