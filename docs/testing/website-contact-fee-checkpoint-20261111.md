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
ACKs. They use generated/synthetic fixtures. Exact locked Rust/clippy/audit results
remain pending CI; no local native pass is claimed.

All nine isolated nginx/contact regressions pass (3.173 seconds), preserving the
existing TLS/LNURL/webhook/WebSocket and burst checks. The actual installed
ModSecurity connector with CRS 4.30.0 also passes all ten isolated acceptance cases
(3.713 seconds), including unchanged signature/body forwarding, injection blocking
and no contact/body/capability sentinel in logs. Initial sandbox socket refusal ran
zero tests; early contact cases exhausted their shared synthetic IP's quota.
Separate loopback client keys isolate the assertions without increasing limits or
weakening WAF rules. Only disposable loopback nginx/mock processes were started.

Still required: paid-contact browser integration and restart/status recovery;
complete enabled HTTP admission tests; native four-journal private-state correlation
checks; a native schema-23-to-24 migration comparison/protected maintenance
transaction and installed rehearsal; current backup and actual deployment;
controlled provider/paid-contact acceptance and recipient-side DM observation.
The old legacy-core upgrade helper is not this upgrade path. No production
configuration, daemon, schema, credential, nginx contact route or journal changed.
