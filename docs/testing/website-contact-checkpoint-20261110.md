# Private contact — source checkpoint, 2026-11-10

The free Nostr contact branch is implemented in source. The anonymous/email
100-sat branch remains unfinished and unavailable; it must not be represented as
an ordinary herd payment. No contact code or financial schema has been deployed.
The active static release remains the separately qualified `9abcd88` website.

The index form reuses the selected extension/Nostr Connect identity. NIP-44
encrypts a kind-14 rumor to the project recipient; that visitor signs the exact
kind-13 seal. A one-use browser key creates the kind-1059 gift wrapper. Native
signature/field checks reject signer substitution or a changed seal; plaintext
cannot silently replace an NIP-44 payload. No private key input or project
credential is introduced. The browser publishes only ciphertext, directly to
relays from the project's authenticated latest kind-10050 inbox announcement.
This branch has no backend fee waiver, invoice or payment-provider operation.

Before relay publication, the exact wrapper and public sender are retained in
browser storage for explicit retry. Plaintext and keys are never persisted.
An uncertain relay result or page reload reuses the same event and does not
request another signature. Another selected account cannot silently claim that
pending message. A relay acknowledgment is described as acceptance, not reading
or an operator reply. Clearing a local pending copy does not retract publication.

Two sandboxed browser tests pass locally (121.176 seconds). They use the shipped
native Schnorr/NIP-44/NIP-59 code and generated fixture keys, with a test recipient
so decryption is possible without the project credential. They verify logged-in
sender/recipient/content after unwrap, anonymous required-email validation,
one-use anonymous identity, changed signer/seal and plaintext refusal, ciphertext
storage, failed relay acknowledgment and exact retry after reload. All relay,
video and provider traffic is isolated; no real message or payment is sent.
The paid branch stays visibly unavailable rather than exposing a nonworking Pay
action. Exact-source CI and actual deployment remain separate.

A read-only query of the actual project's public inbox announcement found
`defe6a8571ae87261278bcae968f88d821e304930e7fad5e7e486f46e1fb20d2`,
advertising nos.lol, Primal and Damus. The installed nak's offline signature check
exited successfully. This discovery used no signer or credential and published
nothing; browser intake still verifies the fetched announcement itself.

## Remaining separate fee transaction

Use the existing receive-only Strike adapter, without adding spend scopes or a
fifth financial journal. Add explicit private contact-purpose state in the core
journal, with an immutable encrypted intent before any provider creation. Bind a
client-generated contact/idempotency identity, hashed status capability, exact
gift wrapper, required email hash, configured recipient/inbox relays and fixed
100-sat description commitment. Validate email transiently; never retain or log
its plaintext on the server. The email is inside the recipient's encrypted DM.

Invoice admission must share the existing bounded issuance budget. Persist an
issuance claim before provider contact. A timeout or crash with an unknown create
result must not automatically issue another request. Retain the original intent
and any issued invoice; an explicit retry returns the same bound result. Strike's
current receive-request creation documentation does not establish an idempotency
header; do not invent one or conflate the older Invoice API's correlation ID.

Commit verified fee settlement and a separate private delivery outbox atomically.
The ordinary `record_payment` path unconditionally creates feed credit, so it
must refuse contact-purpose identities/hashes; a special pool name is insufficient.
Verify both-direction identity collisions with ordinary receives and feed
settlements. No contact fee may create a public event, receipt, reward funding or
feeder debit/credit. Retry the exact encrypted event after relay failure or ACK
loss, including settlement after the browser closes. Status capabilities belong
in bounded POST bodies/headers, never URLs or request logs.

The additive core schema requires a new native upgrade/backup comparison from
the actual installed schema 23. The existing legacy pre-CyberHerd upgrade
transaction cannot simply be reused: it only accepts the earlier legacy prefix.
Keep the current four-journal backup and paid-row/signed-history guarantees,
qualify new purpose/collision/late-settlement/restart/WAF/privacy checks, and
prepare a concrete protected installation before production maintenance.

References: [NIP-17](https://github.com/nostr-protocol/nips/blob/master/17.md),
[NIP-59](https://github.com/nostr-protocol/nips/blob/master/59.md),
[Strike receive creation](https://docs.strike.me/api/create-a-receive-request/).
