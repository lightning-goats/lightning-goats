# Nostr sign-in and operator checkpoint — 2026-11-10

Feature code `04488a445ce60fef666d0838dc487aca2902e4de` was tested as PR merge
`1c9d100853efe81a6d9f837ed375a8d49dea1603` into
`18e0261add12d78b6e567f471bb310ba8aec3898`. Independent local and GitHub Git-data
reads confirmed both trees equal `47eb63ffb465ccba244bb3a80e763e04fb3f0eb1`.
The review archive records the actual tested merge checkout.

[CI 42777274664](https://github.com/lightning-goats/lightning-goats/actions/runs/42777274664),
job `129698621093`, completed format, strict locked all-target/all-feature Clippy
and the full locked all-feature suite: **476 passed, zero failed, one ignored**.
The separate inactive cross-host example passed two tests, excluded from 476.
All ten new Rust regressions are explicitly present in completed logs:

- Native BIP-340 HTTP authentication binds origin/path/query/method, mandatory
  command body hash, time window and pinned operator key. Wrong kind/signature,
  changed body, duplicate/missing tags, stale/future time and malformed encoding
  fail. Canonical public-origin validation rejects private/credentialed URLs;
  GET has no body and cannot be replayed as HEAD.
- Manual commands preserve exact desired totals above the browser safe-integer
  range without minting lifetime payment history. Reopen/lost-response retries
  return the original receipt; consumed authorization events and changed/stale
  intents fail. Bans require separate unban, and social evidence promotes once.
- An injected audit-write failure rolls back state, nonce, command and event;
  exact retry remains usable. Reset keeps replay/lifetime/ban state and leaves
  another owner unchanged. An old committed command cannot reactivate a member
  after a new cycle.
- Zero minimum/member allocation remain zero, capacity cannot implicitly evict,
  paused activation is held, template opt-out survives seeding, unsafe placeholders
  fail and private template reads preserve disabled state.
- Pending confirmed-feeding snapshots and stale core observations hold edits
  without consuming their command identity. A valid retry works after the
  retained barrier completes.
- Manual notices freeze original conversation, human/public wording and changed
  roster footer without a received-payment claim, extra mentions or reset note.
  Empty herds are explicit. Active members with no eligible payout address appear
  at zero share; deleting any snapshot helper opts out of the complete footer.
- Manual reset atomically discards same-second deferred membership work while
  retaining input/source receipts. Injected failure restores the pending work;
  after successful reset/reopen, replay remains discarded and earns no award.
- A real loopback HTTP server and private core Unix server exercise unauthenticated
  and nonoperator rejection, own-key self reads, GET body/HEAD refusal, durable
  nonce replay, tampered body, exact fresh-authorization command retry, revision
  conflict and authenticated core ordering. No core credit, event, Nostr job or
  feeder request is created.

The first candidate failed its new empty-footer assertion, exposing the previous
footer gap. The final implementation adds explicit empty wording, zero-share
active members and full helper opt-out rather than weakening that contract. The
new assertion uses the existing editable legacy empty-herd wording. Subsequent
source review added stale-template revision binding and the manual-reset boundary
regression; the final exact checkout above includes both corrections.

[Deployment 42777274671](https://github.com/lightning-goats/lightning-goats/actions/runs/42777274671)
passed all five jobs. Browser job `129698621717` passed **28 tests**, all HTTP
traffic mocked or blocked. Four new operator tests verify disabled pages make no
API/signing request, nonoperators cannot reveal controls, a lost response retains
one exact command body/UUID while retry signs a new NIP-98 event, and a stale
editor keeps its original template revision after an unrelated refresh. The
browser fixtures use native Nostr-tool signature generation/verification with a
synthetic test key, not a production signer or private-key login.

Package job `129698621770` built all release binaries, passed **ten non-skipped
CLI checks**, and verified a disabled three-binary, **36-file** archive with
`activation=none`. The added CLI check validates pinned HTTPS origin/public-key
allowlist while rejecting private/HTTP origins, duplicate keys and secret fields;
no state is created or credentials loaded. Existing nginx, harmless owner,
package-install and core systemd fixtures passed. They do not establish an actual
installation of new CyberHerd identities, WAF/auth proxy, operator keys or flags.

[Real NIP-46 42777274730](https://github.com/lightning-goats/lightning-goats/actions/runs/42777274730),
job `129698621133`, passed the existing isolated real-nak regression. New browser
and server fixtures do not qualify an actual household client, browser extension
or public relay destination.

[Security 42777274755](https://github.com/lightning-goats/lightning-goats/actions/runs/42777274755),
job `129698621515`, fails solely on unignored **RUSTSEC-2026-0459** in
`libsqlite3-sys 0.30.1`, reported fixed at `>=0.39.0`. RSA remains unreachable;
315 crate dependencies were scanned. The new direct `base64 0.22.1` dependency
uses the version already in the locked active graph. No crate version/checksum
was changed and no advisory waiver was added.

See [the operator interface](../architecture/cyberherd-operator-interface.md).
No new service, proxy, operator identity, credential, real payment, public relay
publication or physical feeding is installed/activated by this source work.
The full port remains incomplete: failed-cost/consistent-restore/retention tools,
dependency remediation, reviewed inactive installation, actual UID/network/helper/
provider/household-signer/browser qualification and explicit activation are still
required. Keep PR111 draft and financial execution disabled.
