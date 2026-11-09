# Native zap receive and protocol receipts

This describes the CyberHerd feature source, not a live activation claim. The
receive-only Strike credential, paid ledger, feeder authority and existing
10-sat announcement floor remain independent of membership and reward workers.
The [full port plan](../planning/cyberherd-implementation-plan.md) remains open.

Protocol references are [NIP-01](https://github.com/nostr-protocol/nips/blob/master/01.md),
[NIP-57](https://github.com/nostr-protocol/nips/blob/master/57.md), and the existing
[address registry](lightning-address-registry.md). Native BIP-340 verification
uses the already locked bitcoin/secp256k1 dependency. Nostr private signing keys
remain in the household bunker; the VPS holds its existing client credential.

## Receive contract

Without `lnurl.zap`, discovery and ordinary callbacks retain their previous shape.
A supplied `nostr` parameter is rejected when support is disabled, rather than
silently returning an invoice with the wrong description hash. Explicit opt-in
configures a receipt public key matching the remote signer, a per-user payee
registry and optional approved alias origins. Unknown users still fail first.

For enabled users, discovery adds `allowsNostr` and `nostrPubkey`. The latter is
the receipt signer, which can differ from a goat's payee identity. The request's
single `p` must match that address's configured payee. All six addresses retain
their actual `address_user` and credit the same herd pool. No NIP-05 lookup is
involved in this authority decision.

Before provider contact the callback validates amount, signed kind 9734, canonical
event ID/BIP-340 signature, timestamp, singleton tags, optional amount/P tags,
event/coordinate syntax, bounded secure receipt relays and any supplied LNURL
binding. The optional bech32 LNURL can name the canonical domain or an explicitly
approved alias, always for the same registered user. Request size is capped at
8192 UTF-8 bytes; edge request lines and daemon transport remain bounded at 32 KiB.
The existing edge/application rate limits and invoice reservations still apply.

The invoice commits to SHA256 of the **exact decoded raw JSON string**, including
its whitespace and ordering. It does not commit to a reserialized event or LNURL
metadata. Ordinary callbacks retain the discovery metadata hash. Issued invoice
and raw request/payee/receipt identity are persisted together, with immutable
context and conflicting duplicate requests rejected. A process interruption before
persistence returns no usable successful callback. Provider reconciliation still
requires the exact persisted signed mainnet invoice, amount and provider source.

Authenticating a requested note/stream's ownership and current membership target
is a downstream CyberHerd decision. Merely referencing another person's event
cannot create membership; it also does not invalidate a real payment to the
project. Relay zap receipts never authorize financial credit or points. The
incoming invoice remains financially valid without profile/discovery availability.

## Settlement and delivery

Settlement, feed credit, `payment_received` and the minimal protocol-receipt intent
commit in one transaction. The separate `zap_receipt_jobs` identity is tied to the
actual provider settlement and source event. It does not reuse or replace the
kind-1 announcement outbox. Repeated notifications do not create extra receipts.
Distinct paid invoices can reuse the same signed request without losing credit.

Strike-optimized P2P/fiat receives continue to credit the authoritative actual BTC
amount. If it differs from the signed invoice's amount, no invoice-paid NIP-57
receipt is manufactured. The future membership bridge must preserve that same
qualification check; ordinary financial settlement is unchanged.

The receipt worker runs only with explicit zap configuration and active Nostr
signing. It reads immutable context after settlement, derives requested relay
destinations, and freezes the settlement timestamp, empty content and tags. It
validates the issued invoice before asking the bunker to sign kind 9735, then
verifies the returned signature and exact intent. Shared client signing calls are
serialized. No spend-capable key is needed for invoice creation or receipts.

Signed bytes are persisted before relay contact and cannot be replaced. Leases
serialize workers; expired ownership cannot overwrite another worker's intent.
Each relay has its own acknowledgement/retry state. Unknown/lost relay responses
retry the same persisted event, without another signature or payment. Signer or
relay outages leave credit intact and receipt work pending. A storage failure in
the atomic settlement transaction leaves no partial credit/event/receipt state.

A 1-sat zap still has its required empty-content protocol receipt. It does not
produce a kind-1 payment announcement or overlay payment message. Weather and
information remain overlay-only. A kind-9734 request is sent only to an invoice
callback and is rejected by the public protocol publisher.

## Untrusted destinations

Wallet-supplied relays and later payout LNURL endpoints use a shared public-network
adapter. It accepts HTTPS/WSS on port 443 without URL credentials or fragments,
rejects private/reserved/documentation/transition addresses and local DNS suffixes,
and bounds DNS, connect and total response times. Any non-public or excessive DNS
answer set fails closed. Validated IPs are passed directly to TCP/HTTP resolution;
TLS still verifies the original hostname, preventing a second DNS lookup from
rebinding into the household or loopback network. HTTP proxy inheritance and
redirects are disabled; JSON body reads and WebSocket headers/frames/messages are
bounded. Relay acknowledgement IDs must match the persisted event.

The TLS transport activates tokio-tungstenite's documented Rustls support. Its
only new locked package is `webpki-roots 0.26.11`, a compatibility wrapper around
the existing `webpki-roots 1.0.10`; existing package versions are unchanged.

## Deployment boundary

Migration 0014 is additive and does not reset paid state, existing invoices,
feeding UUIDs, core cursors or existing signed announcements. Schema upgrades
still require a quiesced backup. Restore the complete paid database rather than
deleting context/outboxes to roll back. Disabling zap discovery retains all paid
intents for later reconciliation; it does not undo payments.

The shipped receive example keeps zap support commented out. Installation and
actual signer/relay acceptance precede activation. Local tests use synthetic
keys, loopback provider fixtures and temporary databases; they do not attest to
live zap receipt publication or physical feeding. Full CyberHerd target ingestion,
membership bridge, payout authority and conversation delivery remain separately
tracked implementation work.
