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
The standalone feed-page wallet regression also passes (85.830 seconds). The first CI website job passed 34 cases but raced the Bootstrap modal transition in one immediate visibility assertion. Waiting for actual visibility fixes the test synchronization; no payment behavior was relaxed. Exact final CI remains to be recorded.

The contact form/separate 100-sat fee is still under implementation. No new
financial schema, paid-contact endpoint, membership publication or payout was
enabled by this website work.

## Qualified static release and actual public acceptance

All ten workflows pass on `9abcd88204b7e9a0fbaf342b3b066785fec9602f`.
The tested merge `13de2a16d4f42e99fd57316b9775affd6e309fec` has zero file
differences. [Deployment run 42924500265](https://github.com/lightning-goats/lightning-goats/actions/runs/42924500265)
passes all 35 browser cases (`130207386664`), nginx (`130207386606`), packaging
and installed systemd qualification. [CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42924500089),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42924500136)
and [real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42924500278)
also pass. Runtime assets are unchanged from `7d456b0`; its failed immediate modal
assertion remains recorded as a failure, superseded by the corrected test.

The active static root is
`/var/www/lightning-goats-releases/9abcd88204b7e9a0fbaf342b3b066785fec9602f`.
The four selected assets independently match pinned GitHub blobs. The actual
previous 27-file release was copied, replacing only `index.html`, `feed.html`
and `site.js` and adding `lightning-payment.js`. The other 24 files, including
all OBS/artwork/vendor/NIP-05 and the live site flags, remain exact. The unfinished
contact source was deliberately excluded from this release.

Release manifest SHA256:
`872cc4727a90bacaf3c1a8533a2ffba156c828c6f28b05b82843981181b3ccf5`.
Private protected preparation:
`/var/lib/lightning-goats-upgrades/website-release-9abcd88`.
Prepared receipt SHA256:
`33eadfdf6575328548c04788b7e04e7c2363d9c920d1049c0261efddb443bb9a`;
nginx transaction SHA256:
`736b1901de6ce8abd3a5d49645b6cef4f7451c234b5bb9324d00637c429237a9`;
installed receipt SHA256:
`d56e1bdae8fafa53af9c791a02d60d310e8d670dd775a06e546a0b62355c4f4f`.
Only the static-root literal changed in the existing nginx site. The authenticated
file transaction retains the original bytes/metadata for configuration rollback;
`nginx -t`, reload and root-owned file readback passed. The first preparation
guard refused incorrect unit names before creating any release; the actual
inventoried `lightning-goats-receive.service` and `cyberherd.service` were then
used. Their PIDs remain `1434299`/`1437779`, with no restart or configuration change.

Eight actual public HTTPS reads pass: the four exact assets, leaderboard, roster
API, herd LNURL discovery and herd NIP-05. Sandboxed Chromium verifies the visible
header leaderboard, absence of the Feed header link, invisible/hit-testable QR
geometry at desktop and mobile sizes, and a working Pay modal. It observes zero
page errors and zero invoice callbacks. Invoice callbacks were explicitly
intercepted during this inspection, so no test payment, grant, DM, Nostr note or
physical feeding was initiated. Reports are retained in
`target/website-release-9abcd88/LIVE-{HTTPS,BROWSER}.json`.

Protocol references: [WebLN payment](https://www.webln.dev/client/send-payment),
[Alby injected APIs](https://guides.getalby.com/developer-guide/developer-guide/alby-browser-extension-apis),
[NIP-57](https://github.com/nostr-protocol/nips/blob/master/57.md).
