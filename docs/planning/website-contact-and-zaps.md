# Website contact and live zaps — operator request, 2026-11-10

This adds to the active full CyberHerd port; it does not replace the twelve-part
contract or authorize CyberHerd payouts/public membership publishing. Preserve the
live receive/feed ledger, owner, OBS and read-only roster while implementing it.

## Accepted behavior

- The `lightning-goats.com` index header links to the new `/leaderboard`. It has
  no NIP-05 or Feed the Herd header link. The video QR/invisible payment layer
  remains available as a fully invisible clickable element with no button label,
  border or focus outline. Its dimensions/position scale with the underlying
  180×180 QR on the 1920×1080 OBS canvas (top 3, right 8), including mobile.
  Existing NIP-05 verification endpoints stay supported.
- Contact becomes a webform delivering a private DM to the existing Lightning
  Goats Nostr identity. **Logged-in Nostr users send for free.** Visitors who do
  not log in must pay exactly **100 sats** before delivery.
- The anonymous/email contact charge is a **separate fee**. It never adds feed credit, funds a
  CyberHerd reward, causes food or enters public payment/overlay/chat messages.
- Offer the existing Nostr extension/Nostr Connect login. When used, the DM must
  cryptographically come from that selected Nostr identity, not an operator or
  proxy identity claiming its name. Do not ask visitors to paste a private key.
- Visitors who do not log in must supply an email address. This supersedes the
  earlier optional anonymous reply-address suggestion. They may still contact
  without having a Nostr account or Nostr-capable wallet.
- Live chat monitors legitimate incoming zaps and renders the sender's human
  name and exact sat amount, for example `Sat has zapped 21 sats.` Deduplicate
  receipt replay, safely render names/comments, and keep private contacts out.

- Payment actions work directly with Alby or another WebLN-compatible extension.
  Generate the invoice internally, then request wallet payment from the same
  user action. Do not expose a separate Create Invoice step. Preserve QR/external
  wallet fallback, cancellation and pending-state recovery. Extension success is
  presentation evidence; authoritative provider settlement still gates delivery.

## Implementation and verification

The actual home page uses a rich inline chat implementation; `web/chat.js` is the
separate simpler feed-page implementation. Tests of the latter do not establish
index behavior. The index also did not load `site-config.js`, leaving its newly
added gated roster link hidden. Verify the actual index, its source settings and
real DOM, rather than assuming a deployed static link is visible.

For chat, inspect the current stream/goal receipt filters, provider-hosted stream
recipient selection, configured receive relays and canonical receipt signer.
The existing filters miss daily-note/direct herd receipts, and the published
receipt relays include Chaima/ArtX. Use native verified receipt/request identities,
authoritative recipient metadata, exact positive sat-aligned amounts and replay
identities. A Nostr receipt must never directly mint financial credit. A normal
chat message containing a payment claim is not a verified zap.

Prefer current NIP-17/NIP-59 encrypted contact delivery. The browser can build a
kind-14 rumor, obtain NIP-44 encryption and an exact kind-13 seal from the selected
NIP-07/NIP-46 signer, then wrap it with a one-use browser key. An unsigned visitor
uses only one-use browser keys and includes the required email inside the private
message. No visitor or project signing key belongs in service configuration,
source, logs, argv or environment. Never silently send plaintext or claim a
logged-in sender when signing/encryption did not prove it.

Free signed contact publishes its NIP-17 gift wrapper directly from the browser
to the recipient's authenticated kind-10050 inbox relays. The selected identity
signs the exact encrypted kind-13 seal and owns the enclosed kind-14 rumor; a
claimed login or caller-supplied pubkey does not establish that identity. No
backend fee waiver, invoice or provider operation is needed for this direct path.
Persist only the exact encrypted wrapper/public sender for explicit relay retry,
never visitor keys or plaintext. A later server-mediated free path would require
fresh payload-bound NIP-98 authorization and durable replay protection.

For the anonymous/email branch, persist the immutable encrypted delivery intent
before requesting payment, bind the invoice to that exact contact and 100-sat
purpose, then use authoritative Strike receive state for settlement. Webhook notifications alone are insufficient.
A contact payment must not pass through the existing feed-credit settlement path.
Its provider/payment identities must remain collision-safe against ordinary
receives. Commit fee settlement and durable private-delivery work atomically;
retries/restarts reuse the original invoice intent and signed encrypted event.
Private message/reply information must not reach public events or request logs.
Bound form/status endpoints, invoices, message sizes, concurrency, lifetime and
unpaid backlog; status access requires an unguessable per-contact capability.

Relay acknowledgement means accepted for delivery, not read by the operator.
Use the recipient's authenticated inbox relay information and retain retryable
exact bytes across relay failure. Close/reload after invoice creation must not
lose an already-paid message. Update native backup/retention and migration checks
for any new durable fee/delivery tables, preserving all existing financial state.

Required evidence includes real index/browser tests; trusted/forged/replayed zap
cases; logged-in signer binding and anonymous email validation; authenticated
amount/purpose and duplicate settlement; no feed/reward/public leakage; delivery
failure/restart/ACK loss; free authenticated delivery with zero provider operations; private ingress/WAF/
log handling; and protected deployment
with current native backup before schema change. Source-only success is not a
claim that the new contact fee or private delivery is live.

Protocol references: [NIP-17](https://github.com/nostr-protocol/nips/blob/master/17.md),
[NIP-59](https://github.com/nostr-protocol/nips/blob/master/59.md) and
[NIP-57](https://github.com/nostr-protocol/nips/blob/master/57.md).

WebLN references: [provider API](https://www.webln.dev/),
[Alby injected APIs](https://guides.getalby.com/developer-guide/developer-guide/alby-browser-extension-apis).
Use `window.webln` permission/payment methods and compatible injected providers;
keep invoice creation behind the action rather than requiring a vendor account
or a new wallet SDK for an already injected extension.
