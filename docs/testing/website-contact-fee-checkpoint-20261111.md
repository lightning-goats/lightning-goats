# Separate contact fee — implementation checkpoint, 2026-11-11

**Source work, not deployed or enabled.** The active core still uses schema 23;
the active website remains the independently verified `9abcd88` static release.
The earlier free-contact source `ae9ec9f` passes all ten workflows, including
[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42926781822),
[CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42926781694)
and [Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42926781796).
Its browser form has not been installed. CyberHerd public publishing and payouts
remain disabled; no paid or physical test is implied by this work.

The additive private-contact implementation now includes schema 24, validated
immutable encrypted intents, hashed status capabilities/email, fixed 100-sat
purpose commitments, durable single issuance claims, authoritative Strike
reconciliation, separate atomic fee/private-outbox acceptance and an exact-byte
NIP-17 relay worker. Neither a special credit pool nor an unverified wallet result
can authorize delivery. The ordinary settlement path explicitly refuses private
fee identities/hashes, including case variants; private fees also refuse existing
ordinary settlements. Public payment/event/zap/reward/feed tables are not written.

Default-off optional `[contact]` configuration binds the project recipient and
bounded trusted inbox relays, requires active mode and adds no signing/spending
credential. Exact POST create/status routes reject URL queries, foreign/missing
origins, unsupported content types and malformed inputs before provider contact.
The 32-KiB create and 1-KiB status limits, four-slot request/issuance admission,
shared durable invoice budget and private-intent capacity remain separate bounds.
No capability, email or private message is accepted in a status URL or diagnostic.

Invoice creation is claimed before the provider request. Interruption or failure
retains `issuing`/`uncertain`; it never receives a fresh provider create on retry.
Known issued invoices and encrypted wrappers remain stable across restart. Fee
settlement and delivery staging commit together. An expired delivery lease or ACK
loss retries the same signed wrapper without signing, fee collection or feeder
credit again. Only a positive matching event-ID acknowledgment from a selected
relay marks relay acceptance; it does not establish that the operator read it.

New native tests cover domain/fee/recipient validation, concurrent issuance claims,
immutable purpose, ordinary/private collision refusal, atomic queue failure,
duplicate settlement, authoritative provider amount/state checks, uncertain create
recovery, stale delivery leases, exact ciphertext retry and positive/negative relay
ACKs. They use generated/synthetic fixtures. All ten workflows now pass on
`822190bab56ad3f2b710162e21114a3f15eeb3d0`; tested merge
`b35b1e7e759ca57722922b0c7c7b7eccc126b298` has zero file differences.
[CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42929632214)
passes locked format/clippy and 511 regular native tests, plus the separately
required root-owned/bootstrap/legacy fixtures. The real-NIP46 fixture runs in its
[separate workflow](https://github.com/lightning-goats/lightning-goats/actions/runs/42929632094).
[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42929632122)
passes website/nginx/package/installed systemd and
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42929632126)
passes. The first Rust attempt passed the new fee tests but hit `Address already
in use` in an existing gateway startup fixture. That failed run is retained; the
failed job alone was retried using the authenticated CLI after the GitHub app
refused rerun permission. The retry passes the unchanged source.

All nine isolated nginx/contact regressions pass (3.173 seconds), preserving the
existing TLS/LNURL/webhook/WebSocket and burst checks. The actual installed
ModSecurity connector with CRS 4.30.0 also passes all ten isolated acceptance cases
(3.713 seconds), including unchanged signature/body forwarding, injection blocking
and no contact/body/capability sentinel in logs. Initial sandbox socket refusal ran
zero tests; early contact cases exhausted their shared synthetic IP's quota.
Separate loopback client keys isolate the assertions without increasing limits or
weakening WAF rules. Only disposable loopback nginx/mock processes were started.

The subsequent browser implementation now connects the anonymous form to these
separate POST endpoints. It requires email, freezes ciphertext/status capability
before creation, stores no plaintext email/message/key, checks the fixed 100-sat
invoice and waits for server status after WebLN. A reload checks the retained
contact without another invoice or wallet submission. Relay acceptance is distinct
from reading. Four isolated contact browser cases pass locally (229.227 seconds),
including native encryption, free identity/retry, anonymous paid status/reload and
wrong-amount refusal before wallet submission. These use mocked financial status,
not an actual Strike fee or recipient-side observation. The combined browser/backend
source `d6f2e79e71295ef2ec4b9edbfba550da307a6a3e` now passes all ten workflows,
including [CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42931655849),
[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42931655609)
and [Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42931655873).
The source flag stays false.

The native source adds an enabled real-HTTP fixture through native invoice
validation and authoritative mock settlement, private-state correlation in ordinary
native four-journal verification, and a separate `compare-contact-upgrade` command.
The comparison accepts only the exact compiled schema-23 prefix and separate
schema-24 four-journal images. It compares every original table/column/typed row,
row ID, signed byte and migration-history prefix; the only allowed additions are
the exact compiled private DDL and three empty tables. It does not migrate,
restore, clear recovery state or authorize resumption. Ordinary verification and
restore continue to require the current schema. Synthetic tests exercise paid
state, pending feeds and signed outboxes plus altered rows, extra triggers,
unknown checksums and prepopulated private tables.

The follow-up adds a required isolated root fixture for the public comparison
entry point, selected and checked for its positive marker by the existing native
CI launcher. It constructs a synthetic preceding-schema manifest, migrates only
a separate copy, verifies preservation of the original manifest/images, and
refuses candidate aliases, hard links, unexpected files, incomplete markers and
publicly readable database files. It does not qualify live unit quiescence or
replace the still-required installed maintenance rehearsal.

These additions pass all ten workflows on
`46257c5e09e4a102346f588c2d1e376a60fddac7`, including
[CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42936833963),
[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42936833946),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42936833993)
and [real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42936834102).
Locked native tests pass 516 regular cases. The new ignored root fixture actually
runs separately and emits `ROOT_CONTACT_UPGRADE_FIXTURE_VERIFIED`; it is not
counted as accepted merely because the ordinary run skips it.

The subsequent [protected preparation tool](../deployment/private-contact-upgrade.md)
freezes the current overlay-only installation and both release archives without
opening journals or stopping services. All ten workflows pass on
`cec3f12f09a5cbd18b94408cd26ab031d13ffbc6`: [CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42937899124),
[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42937899042),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42937899137)
and [real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42937899197).
Tested merge `39a5c6f01893608dec5879cd100117472be1d083` has zero file differences.
The real root-owned preparation/launcher fixtures pass in the required separate
root job. The actual installed systemd rehearsal freezes both authenticated
archives while retaining both running PIDs, then completes the existing rollback
checks. Its retained report explicitly confirms preparation preservation, isolated
networking and zero gateway POSTs. Artifact `13820135187`, ZIP SHA256
`050ebb5f9b711d870e38eafbe50eedf3cb8b3354cbcb85a7e863d73098606acd`,
passes all six internal checksum entries. This does not qualify a live migration.

All eighteen tools independently match pinned GitHub source before inactive
root-owned installation. Manifest SHA256:
`4830749ea5d0e0a212a0c8f1ae34f70234a687a9e608f8f484f01018582d17b0`;
launcher SHA256:
`95d104cb9bdb4350d74cad43e91bcbedfff14971eb92459a6703e195b62a82b5`.
The original seventeen-file tool generation remains untouched.

Actual VPS preparation succeeds against independently verified core/review
artifacts at the tested merge. Core artifact `13819187905`, ZIP SHA256
`dc0dd282161c89b2ae80c3fb54661766ddd607c5fa296ba7029d9bb342fcc05a`,
passes all six inner checksums; core archive SHA256 is
`1aa5c984103741d0887f85f735d2f5df43f0dc25affc0a8d7cd6968b75a5ba16`.
Review artifact `13819980601`, ZIP SHA256
`13160e8522446d942e59c1b41cbbe02c6e0f9cca361e5c7ae9e7a9a109504666`,
passes its inner checksum and 104-file verifier; review archive SHA256 is
`187d2e1e28d7c2859806dcb325267b47e3c804536c265a62dbc89924dd63a8c8`.

Independent readback confirms all seventeen original configuration/unit/program
files retain exact bytes and metadata; the three new programs are only staged.
Preparation record SHA256:
`0fea50e3d7bfeb07117debb526d510ebb43ec13dba2a19668d26c581756f5fb0`;
program transaction SHA256:
`00df8b265c8db55e84312004e52deed68ef5bad4992ded3af04c9a10a2a7bf4b`.
Core/membership retain their original PIDs with zero automatic restarts; payout
and signer remain masked with zero PIDs. Public homepage, leaderboard, roster
API and herd LNURL discovery return 200; discovery still advertises native zaps.
The tool has no apply/resume command. Contact, live schema and credentials remain
unchanged; no test invoice, payment, DM, Nostr post or physical action is initiated.

The subsequent native change adds explicit pre-contact snapshot and source-verification
commands to support reproducible migration rehearsal with one qualified tool.
They retain the ordinary four-writer/file/WAL fences, accept only the exact known
schema-23 source and do not make that bundle eligible for ordinary restore.
The strengthened required root fixture now takes the actual native snapshot from
four distinct-owned synthetic journals instead of constructing its manifest.
All ten workflows pass on `e5317a4734733b40643970819e56a54ecf7dc3e9`, including
[CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42940848504),
[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42940848584),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42940848513)
and [real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42940848518).
Tested merge `f2f48b3830f646af5f9754e16d9b88f209e60b63` has zero file differences.
The strengthened root fixture actually runs and emits its required positive
marker after checking distinct-owned original main/WAL/SHM preservation and
ordinary restore refusal. This newer native binary is not installed on the VPS;
the earlier sealed preparation remains unchanged.

The next source candidate implements the protected maintenance executor's
plan/apply/resume operations. It first verifies a current native four-journal
backup and a separately migrated copy, then replaces only three programs and
compares actual live migration before allowing resumption. It preserves all
configuration, limits and outgoing masks and keeps contact disabled. Failure
fixtures cover native refusal before replacement, changed staged programs,
changed financial state before restart and ambiguous migration without old-binary
rollback. The Python native/systemd boundaries are mocks; root-file CI and an
actual installed migration rehearsal remain required. This candidate is not
installed, and the earlier sealed VPS preparation is untouched.

Still required: exact executor qualification
and installed migration rehearsal; current backup and actual deployment;
controlled provider/paid-contact acceptance and recipient-side DM observation.
The old legacy-core upgrade helper is not this upgrade path. No production
configuration, daemon, schema, credential, nginx contact route or journal changed.
