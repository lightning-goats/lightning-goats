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

The next source change adds an enabled real-HTTP fixture through native invoice
validation and authoritative mock settlement, private-state correlation in ordinary
native four-journal verification, and a separate `compare-contact-upgrade` command.
The comparison accepts only the exact compiled schema-23 prefix and separate
schema-24 four-journal images. It compares every original table/column/typed row,
row ID, signed byte and migration-history prefix; the only allowed additions are
the exact compiled private DDL and three empty tables. It does not migrate,
restore, clear recovery state or authorize resumption. Ordinary verification and
restore continue to require the current schema. Synthetic tests exercise paid
state, pending feeds and signed outboxes plus altered rows, extra triggers,
unknown checksums and prepopulated private tables. This new change still needs
exact native CI qualification; local formatting alone is not acceptance.

Still required: native qualification of these additions; a protected maintenance
transaction and installed rehearsal; current backup and actual deployment;
controlled provider/paid-contact acceptance and recipient-side DM observation.
The old legacy-core upgrade helper is not this upgrade path. No production
configuration, daemon, schema, credential, nginx contact route or journal changed.
