# Read-only roster and repost-hint checkpoint — 2026-11-10

Feature code `46b60d5820d202101b2afa17c888a261c7e773d7` was tested as PR merge
`db6f79388d1aad184cfc9108984c6c06689890d0` into
`18e0261add12d78b6e567f471bb310ba8aec3898`. Independent local and GitHub Git-data
reads confirmed both trees equal `4ddefd0d9d49dc6367c906f62dd9c10678c4b3ae`.
The review archive records the actual tested merge checkout.

[CI 42771139597](https://github.com/lightning-goats/lightning-goats/actions/runs/42771139597),
job `129678964279`, completed format, strict locked all-target/all-feature Clippy
and the full locked all-feature suite: **466 passed, zero failed, one ignored**.
The separate inactive cross-host example passed two tests, excluded from 466.
All eight new Rust regressions are present in the completed logs:

- Optional no-treat hints appear at most once per owner/member/cycle, retain
  exact public/human wording across reopen and `msg:` aliases, and can appear
  again in a new cycle. Marker deletion fails.
- Deleted/disabled hint categories claim no unused slot. A repost earned after
  the payout snapshot suppresses its stale suggestion.
- A failed message insertion rolls back the hint claim and delivery cursor;
  retry commits one exact claim/message.
- Read-only roster projection preserves `9007199254740993` sats and exact points/
  shares without mutating membership. Changed score strings and private pictures
  fail packet validation.
- Actual private IPC imports monotone roster revisions while paused, deduplicates
  a lost ACK, rejects changed stale state/wrong epoch/wrong owner/payout role,
  and cannot replace current state through an old exact retry. Atomic snapshot
  hydration survives empty-cycle reset and reopening, with no food, credit or
  core Nostr job.
- Target observations distinguish unknown discovery from successful empty state;
  stale or failed discovery cannot be revived by an older success.
- The real loopback HTTP server serves 53 members in revision-pinned 50+3 pages,
  rejects invalid/unknown query fields and mutations, refuses stale reports,
  exposes no private source/provider/credential fields and leaves state unchanged.
- Configuration requires explicit nonzero loopback listening even when disabled;
  trusted-network and wildcard binds are refused.

[Deployment 42771139583](https://github.com/lightning-goats/lightning-goats/actions/runs/42771139583)
passed all five jobs. Browser job `129678964769` passed **24 tests**, with all HTTP
traffic fulfilled locally or blocked. Three added browser regressions prove default
flags make no API requests, unsafe names/images stay inert, large sat amounts stay
exact, and empty live/snapshot rosters clear without a food/payment effect. Existing
QR, paused credit/progress, physical-confirmation and replay tests also passed.

Package job `129678964666` built all release binaries, passed **nine non-skipped
CLI checks**, and verified a disabled three-binary, **34-file** CyberHerd archive
with `activation=none`. The added CLI check accepts the optional disabled view
without creating state or reading secrets and rejects wildcard/private/zero binds
and unknown credential fields. The archive includes the two new CyberHerd
migrations and optional inactive nginx snippet. Existing nginx, harmless owner,
package-install and core systemd checks passed; these are not installation or
qualification of the new CyberHerd roles/proxy on the production host.

[Real NIP-46 42771139603](https://github.com/lightning-goats/lightning-goats/actions/runs/42771139603),
job `129678964003`, passed the existing isolated real-nak regression. New presentation
fixtures still use synthetic signatures/relay ACKs, not the actual household client
or public relay acceptance.

[Security 42771139599](https://github.com/lightning-goats/lightning-goats/actions/runs/42771139599),
job `129678964215`, fails solely on unignored **RUSTSEC-2026-0459** in
`libsqlite3-sys 0.30.1`, reported fixed at `>=0.39.0`. RSA remains unreachable.
No dependency/lockfile change or waiver is included.

See [read-only roster/OBS](../architecture/cyberherd-public-roster.md) and
[member delivery](../architecture/cyberherd-member-delivery.md). The API, nginx
snippet, static feature flag and OBS producer remain uninstalled/default-off.
Production paid state, services, configuration, credentials, artwork and messages
were not changed. The full port remains incomplete: browser authentication and
operator settings/template/member/reset tools, explicit failed-cost/consistent
restore/retention operations, dependency remediation, installed UID/network/helper/
provider/household-client qualification and explicit activation remain required.
Keep PR111 draft and financial execution disabled.
