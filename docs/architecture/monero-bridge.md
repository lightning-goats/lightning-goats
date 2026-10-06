# Receive-only home MoneroPay bridge — issue #97

Implementation candidate based on `3fb136eed7824c44321c953d81acc146d2426719`.
Related: [multi-asset plan](multi-asset-payments.md),
[XMR quotes](xmr-quote-service.md), [credit storage](asset-credit-storage.md).
This bridge is separate from the existing OpenHAB/weather gateway and financial
SQLite database. It does not grant feed credit, scan blocks, sign Nostr events,
contact Strike, or command a feeder. No existing daemon route is enabled by it.

## Boundary

```text
VPS quote/payment adapter (#98)
  -> authenticated HTTPS/mTLS over the accepted WireGuard path
  -> dedicated home nginx vhost (project client CA + source restriction)
  -> loopback bridge API (separate bearer token)
  -> loopback MoneroPay (receive + known-address GET + health only)

MoneroPay -> separate loopback callback listener -> coalesced known-intent hint
Bridge poll worker -> authoritative complete receive read -> durable private snapshot

Separate protected sync probe -> atomic root-owned report -> bridge read-only
```

The API listener and callback listener must be different literal `127.0.0.1` ports.
MoneroPay has a third explicit loopback port. The service rejects port 5000 and
listener/provider collisions: the existing weather receiver must remain untouched.
The renderer includes distinct wallet-RPC, node-RPC and mTLS ports as deployment
inputs. Example ports are **not** reservations or assertions about the home host.

Only these authenticated API routes exist:

| Method/route | Meaning |
| --- | --- |
| `GET /healthz` | Process/API availability; not chain synchronization or receipt finality |
| `POST /v1/receives` | Reserve a project intent and, once only, request a MoneroPay subaddress |
| `GET /v1/receives/{intent_id}` | Bounded private cached evidence with explicit freshness/readiness |

No balance, transfer, delete, generic proxy, wallet/node RPC, user-selected URL,
callback destination, arbitrary address lookup or administrative route exists.
The callback route is not installed on this API router or forwarded by nginx.
API authentication uses a separate 32-random-byte lowercase-hex bearer token.
Credentials go in protected files/systemd `LoadCredential`, not command lines,
nginx literals, repository files, public URLs, browser code or evidence bundles.

The supplied nginx example uses a **dedicated project-client CA** with client
verification required. Only the reviewed VPS client certificate belongs to that
CA. A shared home-user CA is not equivalent. Verify server identity from the VPS;
never disable TLS verification. Source IP filtering is defense in depth, not
cryptographic peer identity. Preserve the approved home/hub containment and recovery
access; the example does not establish that real network policy by itself.

The optional UID-scoped outbound nftables example permits the bridge's provider
connection and established loopback API/callback replies only. It does not alter
other UIDs, forwarding, NAT or household inbound rules. Systemd's localhost limit
alone does **not** isolate individual local service ports. Review/render/test both
host policy and this rule before exposing the mTLS edge; do not flush a ruleset.

## Durable create/retry contract

`POST /v1/receives` accepts exactly:

```json
{
  "intent_id": "d2a70c8d-558c-4ffc-9a17-ed4e5bd83720",
  "expected_atomic": 1000000000,
  "address_user": "nova",
  "expires_at": 1789500000
}
```

Values above are illustrative, not a live quote. The caller obtains the amount
and expiry from the immutable quote service; quote validation remains in #98.
There is one configured wallet/network/account namespace and the existing six-goat
allowlist. Unknown fields, nonpositive/out-of-range amounts, nil UUIDs and new
expired/out-of-policy requests fail before provider creation.

A SQLite `BEGIN IMMEDIATE` transaction durably saves the canonical request,
creation reservation and private callback token **before** the only upstream POST.
Same ID and identical fields replay the stored result; changed fields fail.
Creation count/hour and retained-intent caps are shared through SQLite. A separate
bounded semaphore prevents an unbounded queue of upstream work.

MoneroPay's inspected creation API has no idempotency key. Its source creates a
wallet address before finishing its database transaction. Consequently any lost,
malformed, unsuccessful or uncommittable creation result remains `creation_unknown`;
process cancellation can leave `creating`. Neither state is permission to POST
again. No automatic reconciliation or administrative repair endpoint is provided
for unknown creates in this slice. Reconcile those privately with the provider
mapping before exposing any recovered address. Never delete the intent to retry.

The bridge verifies an echoed amount/description and a checksummed subaddress for
the configured network. A full authoritative receive read must verify its metadata
before an address is first exposed. If initial readback fails, the binding remains
stored for polling; the address is withheld. A second ID cannot bind the same
address. Once exposed, an address remains in status even when readiness is lost:
an expired or unavailable UI must not pretend received funds disappeared.

## Snapshot and the #98 adapter

Snapshots contain `version`, durable `generation`, `intent_id`, `network`,
`account_scope`, `expected_atomic`, creation `state`, optional `address`,
`observed_at`, `revision`, `ready`, optional `hold`, and private `receipts`.
Each receipt carries the MoneroPay transaction fields and bridge `first_seen_at`.
All atomic amounts and timestamps remain integers. JavaScript consumers must not
round large JSON integers; #98 must parse these in the trusted Rust adapter before
constructing a separately bounded browser projection.

`ready=true` means a fresh synchronized full receive observation passed this
bridge's checks. It is **not** a credit grant and does not mean every receipt is
unlocked, timely or within the quote's credit limits. #98 must verify expected
version/generation/intent/network/account/amount/binding, freshness and no hold,
then reconcile **the entire snapshot** against its previous authoritative history
before allocating any receipt. Missing/contradictory history cannot be processed
as an ordinary partial page. Apply quote-time policy separately to each receipt;
only eligible unlocked amounts enter the existing atomic credit transaction.

An intent-scoped identity includes provider/network/account/subaddress and tx hash.
The same tx may legitimately appear under distinct project subaddresses. Within
one receive, duplicate tx hashes are rejected rather than blindly counted; the
selected upstream wallet's aggregation semantics still require conformance testing.
Do not treat the tx hash as a project-wide unique payment identity.

Callbacks contain a bridge-generated private path token and terminate locally.
Their bodies are ignored: amounts, timestamps and purported completion never
become evidence. They set one durable coalesced hint, not an expanding message queue.
The worker uses fair oldest-attempt polling regardless of callback delivery. This
version deliberately does not forward notifications to the VPS; #98 polls known
IDs. Callback hints cannot jump ahead indefinitely and starve older intents.

Polling includes paid and expired bound intents. Per-intent durable leases fence
stale concurrent read results, and failures clear availability while retaining
old evidence. Successful full reads require exact total/unlocked coverage and
consistent completion, metadata, unique IDs and bounded transaction count.
Double-spend flags, changed/missing history, relocking, falling confirmation count,
changed mined height or wallet-height regression result in a permanent private
hold. There is no automatic hold-clear, re-credit, refund or physical rollback.
Malformed/unavailable transport stays unavailable and is retried, not trusted.
A receive read ahead of the most recent independent wallet-height report waits
for a newer report rather than permanently holding an otherwise valid intent;
no such unverified observation is committed or exposed as ready.

First-seen time is the bridge's successful **synchronized read/commit** time, never
a callback, sender or block timestamp. An outage may therefore make timing
unprovable and a late receipt ineligible under the quote. That conservatism is
intentional; don't backdate it. Preserved first-seen evidence survives unlock and
restarts. A treasury sweep cannot reset it: the bridge never reads wallet balance.

Retained history is never silently evicted to regain capacity. These caps bound
resource use, not justify unlimited production lifetime. Polling currently handles
up to 16 oldest intents sequentially per pass, then sleeps `poll_seconds`; pending,
paid and expired mappings all cost work. Size retention, per-receive bounds,
upstream deadlines, quote lifetime and freshness together. Measure actual poll lag
before activation; reaching capacity requires a reviewed retention design, not
history deletion or an increasingly stale status displayed as ready.

## Sync report: evidence, not another wallet scanner

Pinned MoneroPay `/health` checks database access and calls wallet `refresh`.
It does not independently establish that the node is online/current or that a
wallet's scan height matches it. The bridge therefore also requires a protected
fresh `monero-sync-v1` report. The bridge has **no** RPC credential or RPC client.

`deploy/monero/sync-probe.py` is a separate short-lived protected producer. It only
calls node `get_info`, wallet `get_address` (account 0), wallet `get_height`, then
node `get_info` again at fixed loopback endpoints, with optional file-based HTTP
Digest credentials. It rejects redirects/proxies, malformed/oversized data,
wrong network, unsynchronized/offline/untrusted/busy state, absent peers, excessive
wallet lag, changed wallet fingerprint and clock anomalies. It atomically emits
an unavailable report on failure. The service time budget also bounds a stalled
probe; a stale report cannot make a snapshot ready.

**Deployment must establish that MoneroPay uses that exact dedicated wallet and
node.** The report's configured provider URL and wallet fingerprint do not prove
MoneroPay's private RPC configuration. Inspect it locally and retain a sanitized
acceptance record. This version targets a local node with online peer evidence;
it does not silently substitute an unverified remote daemon.

Use a dedicated **view-only project wallet** after verifying the selected stack
can create/monitor its subaddresses. Keep its spend key off the receiver host.
A receive-only Rust route is not a sandbox against code execution: MoneroPay also
has spending APIs, so a hot wallet behind it would change the authority model.
View keys, provider data, receiving addresses and histories remain private even
when no spend key is present. The probe's RPC credential must not be given to the
bridge UID or the public VPS daemon.

## Deployment and acceptance

See [the inactive preparation guide](../../deploy/monero/README.md). The new binary
is explicitly registered in Cargo, but this slice does not alter the existing
three-binary release packager. Build and stage the bridge separately with the
included digest-checking helper; integrating final combined releases remains #100.

Offline tests use temporary SQLite databases, synthetic reports and a mock
MoneroPay. The binary restart regression runs the actual new bridge executable,
not your home service. They do not establish installed MoneroPay behavior, live
mTLS/firewall/systemd confinement, a real wallet sync chain or #98 settlement.
Retain independent code review, full repository CI on the final candidate, and
source-pinned home/VPS conformance before enabling Monero. Strike remains separate.

## Inspected upstream reference

MoneroPay commit `cbf644ce025914857fdafa8e7e7aeacb849c6159`:
`pkg/model/receive.go`, `pkg/model/health.go`,
`internal/server/controller/receive.go`, `internal/daemon/payment.go`, and
`internal/daemon/health.go`. DTOs intentionally fail closed on unexpected fields.
GET is unfiltered: no min/max query is accepted. Null transaction slices represent
no receipts; partial callbacks are not GET histories. A pin in this document is
an inspection target, not proof of which binary runs on the home server.

Subaddress prefixes/encoding use Monero mainnet 42, testnet 63, stagenet 36 and
block Base58 plus four-byte legacy Keccak checksum. The local checksum code is
for public address typo/network validation only, not signing or key derivation;
known public address and Keccak vectors are regression-tested.
