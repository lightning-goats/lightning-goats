# Website payments and zap display — source checkpoint, 2026-11-10

The index header/QR change `6c02552` passed all ten workflows, including
[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42921156238)
and its actual index browser geometry check. That source was not independently
activated before the following combined website correction was prepared.

The new shared browser payment adapter creates the native LNURL invoice behind
the **Pay** action and sends it through an injected WebLN wallet, including
Alby's compatible provider. Lightning payment requires no Nostr login. A visitor
already connected to Nostr signs an exactly bound zap request; its failure never
silently substitutes another identity. The current goal is included only when
selected. There is no separate Create Invoice step. Without an injected wallet,
the same action reveals a QR and external-wallet link.

Extension permission is requested before invoice creation. Refusal creates no
invoice; an uncertain send retains the existing invoice for an explicit retry.
No automatic send retry occurs. A wallet response does not establish provider
settlement or physical feeding. Both index and feed-page payment paths use the
bounded same-origin LNURL callback, exact integer amount and existing backend
invoice verification. The feed page also adopts the invisible, responsive OBS QR
hit area, without a label, border or hover decoration.

Index receipt discovery now includes direct/daily-note herd zaps and the configured
Chaima/ArtX relays. A displayed zap requires a signed receipt from the canonical
LNURL receive signer, a valid signed kind-9734 request, matching recipient and
optional sender tag, positive whole-sat invoice amount, exact optional amount and
LNURL bindings, and unambiguous critical tags. Replayed invoices render once,
including alternate signed receipt IDs. The goal tally uses the same validation
and invoice deduplication. These display attestations never create ledger credit.

Arbitrary foreign livestream host tags no longer select a room. Foreign events
must exactly match the fresh same-origin authenticated target projection; linked
goals must be project-authored and match that projection. Project-authored streams
remain independently discoverable. Names/comments render as text, with bounded
names and safe avatar schemes.

Three actual index browser tests passed locally (147.244 seconds) with all wallet,
relay, invoice and video traffic isolated. They cover the invisible target's Pay
flow, alternate Alby provider, permission refusal before invoice creation,
QR/external-wallet fallback, cancellation/uncertain response and exact invoice
reuse. The receipt case uses the shipped real Schnorr implementation for both
request and receipt signatures and refuses forged issuer/content, wrong recipient,
amount mismatch, wrong request kind, duplicate critical tags and future receipts.
It renders `Sat has zapped 21 sats.` once and treats an HTML comment as text.
Synthetic BOLT11 strings exercise the client display parser, not invoice settlement.
The standalone feed-page wallet regression also passes (85.830 seconds). Exact final CI remains to be recorded.

The contact form/separate 100-sat fee is still under implementation. No new
financial schema, paid-contact endpoint, membership publication or payout was
enabled by this website work. Production release and actual public-browser
acceptance must be recorded separately after exact-source qualification.

Protocol references: [WebLN payment](https://www.webln.dev/client/send-payment),
[Alby injected APIs](https://guides.getalby.com/developer-guide/developer-guide/alby-browser-extension-apis),
[NIP-57](https://github.com/nostr-protocol/nips/blob/master/57.md).
