# Historical incoming-zap observation — 2026-11-14

Read-only current journals contain eight settled zap receipt jobs, eight signed
receipts and relay acceptance for every receipt. There are 48 request contexts;
unsettled requests are not treated as paid zaps. No provider request, new payment,
feeding, membership edit or public message was submitted for this observation.
Protected installed-file guards passed before/after.

Offline `nak verify` accepted both the kind-9734 request and kind-9735 receipt
for all eight settlements. Exact original request UTF-8 hashes match the stored
invoice description hashes; receipt description tags retain those exact bytes,
bolt11 tags equal the stored invoice, receipt signing identities equal the pinned
receipt keys, and signed payee/optional amount tags agree. Stored invoice and
settlement payment hashes and amounts match. The observation does not independently
reverify BOLT11 signatures or refetch authoritative provider state. No receipt
contains a preimage; no preimage verification is claimed. No invoice, request,
preimage, callback or credential bytes were emitted in the sanitized report.

Each settlement maps to exactly one completed ordered membership input. Two have
one applied interaction and one domain event each: a join and a displacement.
The remaining six have discarded interactions and no domain event. None has a
duplicate input/interaction/domain event. All eight predate today's cycle:
applied payments occurred November 11–12; discarded payments November 13.
Current cycle 5 starts November 14 at 07:00 UTC (midnight America/Denver).
The current source discards pre-cycle or stale-target interactions; this observation
does not distinguish which historical discard condition applied at processing.
It does not replay those inputs or give them today's membership eligibility.

Protected sanitized report:
`/var/lib/lightning-goats-upgrades/zap-settlement-observation-20261114/RESULT.json`,
SHA256 `426e3361a66980f7370d2e1a31c904e065cf7a9b2dd500024f68d36c06f626d5`.
Requests/receipts were verified only from retained journals; relay acceptance
means retained relay ACKs, not an independent relay fetch or viewer observation.

This supplies historical paid-zap/join/displacement evidence previously missing
from the acceptance inventory. It does not prove a new zap on the latest deployed
programs, sender-aware public copy introduced afterward, active-member increase,
all rejection/displacement cases or an actual payout. Payout readback still has
zero execution intents/payment bindings/observations and zero reserved reward
plans; the two existing plans remain no-payout plans. These gaps remain open in
the [full acceptance inventory](cyberherd-acceptance-matrix.md).

## Subsequent independent relay observation

Bounded read-only kind-9735/known-ID queries retrieved matching signed event
objects from Artx (four receipts), nos.lol (four), and Primal (five). Received
objects matched the retained signed receipts; nak's default signature verification
remained enabled. Damus's query failed and establishes neither presence nor
absence. Per-relay counts are not summed: overlap was not retained in the sanitized
report, so this observation does not prove independent retrieval of all eight.
No authentication, publication, provider request or payment operation was requested.
The Nak subprocesses used new isolated configuration directories with cleared
environments; protected installed-file and Nak-binary guards passed before/after.

Protected initial relay report:
`/var/lib/lightning-goats-upgrades/zap-receipt-relay-observation-20261114/RESULT.json`,
SHA256 `0d4c106a024db4a3c0a5dd9a7c32abfed4e1fdfdf80d01c456c9ed1279946532`.
Protected follow-up report:
`/var/lib/lightning-goats-upgrades/zap-receipt-relay-observation-20261114-followup/RESULT.json`,
SHA256 `e5caecfe99d12d3b05ee4521c7e8237c4efda7ea529176bf8a4acf6505c4fc9f`.
This supplies independent historical relay visibility for the retrieved subset;
it does not establish current-release payment-copy or payout acceptance.
