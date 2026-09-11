# OpenHAB Feeder Gateway Security Design

Status: required Phase 1 architecture. Tracker: #17; weather: #21.

## Purpose

The public VPS and `lightning-goatsd` must not hold an OpenHAB API token or have broad access to the in-house OpenHAB/LAN environment. Phase 1 enforces least privilege with a purpose-built trusted gateway on the existing WireGuard network plus host firewall policy.

## Existing network

```text
10.8.0.0/24
10.8.0.1   current/old VPS WireGuard hub
10.8.0.6   in-house OpenHAB + weather host
```

During parallel staging, the new VPS gets a new keypair and an inventoried unused temporary `10.8.0.x`. It must not claim `10.8.0.1` while the old hub is active.

## Existing feeder authority must be reused

A 2026-08-20 live-system audit in `santyr/Solar_PV` found that OpenHAB rule `88bd9ec4de` is already the correlated feeder owner. It is triggered by commands to:

```text
GoatFeeder_ManualRequest
```

and the deployed owner maintains a persisted request ledger/result Item for completed UI requests. Its legacy `/runnow` compatibility path is explicitly **not** authoritative proof of physical completion.

Therefore Phase 1 should **reuse the existing correlated owner**, not create a second competing physical feeder owner, unless a new live audit proves that contract has changed.

Before configuring production, Codex must inspect the deployed source and OpenHAB runtime on `10.8.0.6` and record:

1. current SHA-256 of the deployed feeder-owner script;
2. exact request Item (expected `GoatFeeder_ManualRequest`);
3. exact persisted correlated result Item;
4. exact request command payload shape;
5. exact successful result JSON/state shape;
6. the owner's current duplicate/rate/safety behavior.

The historical audit recorded rule-script SHA-256:

```text
730053e0f3245cb83461e3fe6e3b05d49c8b508631e8cdb4a889c8be8d915978
```

Treat that as historical evidence, not a value to force. If the live SHA differs, inspect and document the change before proceeding.

## Trust boundary

```text
Internet
   |
   v
new VPS / lightning-goatsd
   |
   | 10.8.0.0/24 WireGuard
   | UFW: gateway port only
   | NO OpenHAB token
   v
10.8.0.6 :8789 lightning-goats-gateway
   |
   +-- dedicated OpenHAB USER token -> loopback OpenHAB REST
   |      reads FeederOverride
   |      reads LightningGoatsRemoteEnabled
   |      writes only correlated feeder request Item
   |      reads only correlated result Item
   |      optional temperature read
   |
   +-- local read-only weather -> 127.0.0.1:5000/get_received_data
   |
   v
existing OpenHAB feeder owner -> physical feeder
```

A complete compromise of the VPS should yield neither a generic OpenHAB credential nor general trusted-side network access.

## Dedicated OpenHAB identity

Create a dedicated OpenHAB USER account/token for the gateway. The token:

- exists only on `10.8.0.6`;
- is injected with systemd credentials;
- is never copied to Vultr or committed to Git;
- is rotated if the gateway host/deployment path is suspected compromised.

OpenHAB USER authorization is coarse, so the effective least-privilege boundary is the gateway code + loopback-only OpenHAB client + systemd/UFW containment, not a claimed per-Item token scope.

## Gateway API

F03 wire contract: feeder POST and GET return bounded JSON containing the exact
`request_id` and a typed `status`: `not_dispatched`, `pending`, `confirmed`, or
`ambiguous`. Only correlated `confirmed` with HTTP 200 permits a debit. Bare
204 responses, unknown statuses, wrong UUIDs and contradictory refusal fields
are untrusted outcomes. Upgrade daemon and gateway together; an older gateway
leaves the new daemon safely unresolved until a compatible status lookup works.

`not_dispatched` includes a typed refusal reason (`safety`, `unresolved`, or
`capacity`) and `retry_after_seconds`. It is an immutable gateway tombstone:
that UUID can never subsequently dispatch, including when another process
observed different safety state. Refusals do not consume physical cap history.
The daemon preserves credit, records `feeder_not_dispatched`, and commits its
cooldown in the same transaction. A fresh UUID is allowed only after that
durable cooldown. Tombstones and cooldown must be included in backups.

Pending/ambiguous attempts, interrupted intents, and failed debit commits are
reconciled by GET of the original UUID. GET 404 never proves non-dispatch. No
network error clears ambiguity. POST has a hard 140-second gateway envelope
and 150-second client budget; status GET has 10/15-second gateway/client budgets.
The acknowledgement timeout includes each poll read and sleep. Deadline expiry
retains uncertainty; it never releases a reservation. Ordinary canary examples
use a five-second inter-feed delay matching the gateway's five-second minimum.

These internal protocol tests use the harmless UUID-echo owner only. F04's
actual owner receipt/rejection/completion semantics remain unverified.

The implemented gateway exposes only:

```text
GET  /healthz
GET  /v1/feeder/override
GET  /v1/temperature
POST /v1/feeder/request/<uuid>
GET  /v1/feeder/request/<uuid>
GET  /v1/weather
```

It is not an OpenHAB reverse proxy. Unknown paths/methods are not forwarded.

Production listener:

```text
10.8.0.6:8789
```

A separate canary listener on `10.8.0.6:8790` must use harmless canary request/ack Items and must never target the physical feeder owner.

## Correlated request compatibility

The inspected owner protocol is documented in [openhab-owner-contract.md](openhab-owner-contract.md).
The production candidate uses:

```toml
request_item = "GoatFeeder_ManualRequest"
ack_item = "GoatFeeder_ManualResult"
protocol = "feeder_request_v1"
```

The gateway serializes JSON `requestId` and a fresh UTC `requestedAt`; only the
exact correlated `complete/complete` result confirms the owner's sequence.
Progress remains pending; owner rejection/failure never authorizes a fresh UUID.
Generic success aliases and configurable command templates are removed. The
explicit `uuid_canary` echo protocol is confined to the two harmless canary Items.
Lost-result recovery from authoritative persistence history remains an open gate;
a current Item-state snapshot alone is not proof of persistence completion.

## Exactly-once and ambiguity

F02 remediation: every gateway process for the same owner must share the same
file-backed SQLite database on local durable storage. Admission uses a single
`BEGIN IMMEDIATE` transaction for exact-ID lookup, the global pending guard,
interval/hour capacity checks and request reservation. Only its committed winner
may issue an OpenHAB command. Stop all older gateway binaries before upgrading;
separate databases or old binaries are not a supported multi-instance topology.

Any pending UUID blocks every new UUID indefinitely, including after restart,
command timeout or failed acknowledgement persistence. Existing databases with
multiple pending UUIDs are preserved and remain blocked until each resolves.
Completion timestamps conservatively anchor cooldown/hour capacity. Same-ID
POST/GET reconciliation never sends a command; successful reconciliation and
reservation each append a durable `feeder_request_events` row in the same
transaction as the state change. Replays do not duplicate audit rows or move
completion timestamps. No public endpoint clears pending state as "not fed".

This addresses shared-store admission, not F04's still-unverified physical
completion contract. The typed adapter is not final live acceptance; see the
remaining persistence-history gap in `openhab-owner-contract.md`.

`lightning-goatsd` and the trusted gateway use the **same feed-attempt UUID**.

1. `lightning-goatsd` commits feed intent UUID `X` in its durable ledger.
2. Gateway persists `X` as `pending` in its own SQLite database **before** sending any OpenHAB command.
3. Gateway submits the rendered correlated command to the existing owner.
4. Gateway confirms only after the result Item authoritatively identifies `X` as successful.
5. Gateway persists `X=acknowledged`.
6. Only then does `lightning-goatsd` commit its feed debit/`feeder_confirmed` event.

A persisted pending UUID is never automatically re-commanded, including after gateway restart. If a response is lost, the same UUID can be queried for later authoritative acknowledgement but cannot actuate twice through the gateway.

Any ambiguous gateway/network error causes `lightning-goatsd` to mark its attempt `unknown` and block automatic feeding until reconciliation.

## Local safety layers

For every **new** feed UUID, the trusted gateway requires:

- `FeederOverride == OFF`;
- `LightningGoatsRemoteEnabled == ON`;
- local persisted minimum-feed interval elapsed;
- local rolling `max_feeds_per_hour` not exceeded.

Production examples currently use:

```text
min_feed_interval_seconds = 30
max_feeds_per_hour = 10
```

These are defense-in-depth values and must be reviewed against the actual feeder/animal-care policy before deployment. The existing OpenHAB owner remains final physical authority and its own safety logic must not be weakened.

## Remote kill switch

Create/verify a local Switch Item:

```text
LightningGoatsRemoteEnabled
```

Production must start with this **OFF**. It is enabled only for the explicitly approved physical canary/cutover step. `FeederOverride` remains an independent existing control.

## Weather path

The legacy service on `10.8.0.6:5000` exposes both:

```text
GET /get_received_data   # read
GET /weather?...         # mutating ingestion
```

The VPS must never access port 5000 directly. The gateway alone reads:

```text
http://127.0.0.1:5000/get_received_data
```

It validates payload size/types/ranges/staleness and exposes only sanitized `GET /v1/weather`. Weather failure affects presentation only, never payment/feed accounting.

## WireGuard/UFW

Use `deploy/ufw/lightning-goats-gateway.sh.example` as a reviewed template. Do not flush or replace unrelated firewall policy.

During staging, allow only the new temporary VPS `10.8.0.x` to `10.8.0.6:8789` and (while canary is needed) `:8790`.

At cutover, after the old hub is stopped, replace the temporary source with production `10.8.0.1 -> 10.8.0.6:8789`.

Required negative tests from the VPS:

```text
10.8.0.6:5000   blocked
10.8.0.6:8080   blocked
10.8.0.6:22     blocked unless separately/operator-approved
10.8.0.6:5432   blocked
unrelated LAN/WireGuard hosts/ports blocked
```

Do not rely on WireGuard `AllowedIPs` alone as authorization.

## System services

Gateway runtime identity:

```text
lightning-goats-gateway
```

Use the supplied system-level units:

```text
deploy/systemd/lightning-goats-gateway.service
deploy/systemd/lightning-goats-gateway-canary.service
```

They use systemd credential injection, no capabilities, filesystem protections, and `IPAddressDeny=any` with only localhost + `10.8.0.0/24` allowed. UFW provides peer-specific ingress authorization.

The VPS `lightning-goatsd` units intentionally contain **no OpenHAB credential**.

## Staging gates

Before any physical feeder test:

1. inspect and document live correlated owner contract;
2. create dedicated OpenHAB USER/token;
3. create `LightningGoatsRemoteEnabled`, default OFF;
4. deploy production gateway with remote feeding still OFF;
5. deploy canary gateway with harmless canary Items;
6. verify canary duplicate UUID produces exactly one harmless action;
7. verify gateway restart with pending UUID does not resend;
8. verify `/v1/weather` and overlay weather formatting;
9. run all UFW positive/negative tests;
10. only with operator approval, enable remote feeding and run one controlled physical UUID test;
11. replay/query the same UUID and prove no second actuation.

## Phase 2

CyberHerd must not bypass this gateway or acquire OpenHAB credentials. Future feeder-producing logic must use the same durable feeder authority/boundary.
