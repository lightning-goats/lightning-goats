# Phase 1 Verification Matrix

Audit addendum (2026-09-10): production is on HOLD. The 2026-09-08 findings and
`../deployment/audit-remediation.md` add required negative, concurrent, delayed,
missing-notification and restart cases. Existing always-successful gateway mocks
do not satisfy the combined daemon -> real gateway -> mock OpenHAB gate.

Artifact checks now run via `python3 -m unittest discover -s deploy/tests -v`
and the `Deployment artifacts` workflow. They cover nginx TLS assembly/routing
and complete release archives, not payment settlement or physical completion.

Status: required before production cutover.

Tracker: issue #15.

This document converts the Phase 1 security and functional requirements into explicit acceptance checks. Codex may automate these checks, but real DNS/WireGuard cutover and physical feeder actuation remain operator-gated.

## 1. Build / static verification

Required on the release candidate:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
```

Run the repository security/audit workflow documented in `docs/implementation-status.md`.

Record:

- Git commit SHA;
- release tag if used;
- Rust toolchain version;
- SHA-256 of deployed binaries;
- config/template revision.

## 2. Lightning Address registry

Required configured users:

```text
herd
dexter
rowan
cosmo
newton
nova
```

For each configured user verify:

- discovery endpoint succeeds;
- callback URL references the same canonical user;
- min/max amounts are correct;
- exact metadata string is stable;
- callback creates a Strike BOLT11 invoice;
- completed payment records the correct `address_user`;
- payment credits `credit_pool=herd` exactly once.

Negative tests:

- uppercase/noncanonical user;
- path traversal / encoded separator attempts;
- unknown user;
- empty user;
- overlong user;
- amount below minimum;
- amount above maximum;
- malformed amount.

For unknown/invalid users assert **zero Strike API calls**.

## 3. LNURL metadata / BOLT11 contract

Verify:

- callback amount is millisatoshi-safe and sat alignment policy is explicit;
- metadata used for discovery and `descriptionHash` is byte-for-byte identical;
- returned BOLT11 commits to the expected description hash;
- callback error paths produce LNURL-compatible safe errors.

## 4. Public abuse controls

### LNURL callback

Verify nginx and application limits independently:

- normal wallet traffic succeeds;
- burst invoice creation is throttled;
- discovery can have a higher limit than invoice creation;
- provider is not contacted after application-level rejection;
- rate-limit logging contains no secrets.

### Strike webhook

Verify rejection of:

- GET/PUT/etc.;
- wrong content type;
- oversized body;
- invalid HMAC/signature;
- malformed JSON;
- unsupported event type.

Verify no financial side effect occurs for rejected requests.

## 5. Strike settlement

Test:

- receive-request creation success;
- provider 4xx error;
- 429/rate limit;
- 5xx;
- connect timeout;
- read timeout;
- malformed provider JSON;
- webhook points to pending/incomplete receive;
- completed receive;
- duplicate webhook;
- reordered webhook;
- replay after restart;
- same source ID with conflicting data;
- same payment hash with conflicting source data.

Requirements:

- webhook itself never directly credits;
- authoritative provider read occurs before credit;
- duplicate/replay is idempotent;
- conflicts fail closed;
- settlement + credit + `payment_received` event commit atomically.

Verify the production runtime Strike key cannot spend/withdraw.

## 6. Feeder accounting

Canonical test:

```text
2340 sats received
-> feed credit 2340
-> feed #1 confirmed
-> feed credit 1340
-> feed #2 confirmed
-> feed credit 340
-> no third feed
```

Test:

- one threshold exactly;
- multiple thresholds;
- remainder;
- threshold configuration validation;
- restart before feed;
- restart after intent but before ack;
- unresolved feed blocks subsequent automatic feed.

## 7. In-house integration gateway / OpenHAB

Follow `docs/security/openhab-feeder-gateway.md`.

Production host context:

```text
10.8.0.6   OpenHAB/weather/integration host
```

### Positive feeder path

- gateway health reachable from VPS;
- override read works;
- optional temperature read works;
- unique request UUID accepted;
- OpenHAB local rule evaluates safety gates;
- authoritative ack for UUID returned;
- one ack causes one durable feeder debit/event.

### Duplicate/replay

Send the exact same feed UUID twice.

Requirement: physical feeder actuation count remains one.

### Safety gates

Test at least:

- `LightningGoatsRemoteEnabled=OFF`;
- `FeederOverride=ON`;
- override invalid/unavailable;
- minimum feed interval not elapsed;
- absolute feed cap reached;
- malformed UUID;
- unknown request UUID query.

No rejected condition may actuate the feeder.

### Ambiguity

Inject loss/timeout after a request may have reached OpenHAB.

Requirement:

- daemon marks attempt unknown or remains pending for authoritative same-UUID lookup;
- never submits a new actuation UUID automatically;
- later same-UUID acknowledgement may reconcile safely;
- otherwise operator reconciliation required.

## 8. Weather overlay compatibility

Follow `docs/architecture/weather-overlay.md` and issue #21.

Existing trusted source:

```text
http://10.8.0.6:5000/get_received_data
```

But the VPS must use only:

```text
integration gateway GET /v1/weather
```

Verify:

- gateway can read the legacy weather source locally/trusted-side;
- normalized response includes correct available temperature/humidity/wind/UV and optional pressure/rain/solar values;
- invalid types/out-of-range fields are rejected/omitted safely;
- empty response is safe;
- malformed response is safe;
- stale response policy is enforced when timestamp parsing is available;
- weather failure does not affect payment/feed state;
- weather scheduler interval is configurable (initial default 60 seconds);
- broadcast probability is configurable (initial default 0.30);
- deterministic tests control randomness;
- `weather_status` renders in the expected `🌤️ Weather Update:` style;
- `weather_status` goes to overlay only;
- weather never enters Nostr outbox.

Security negative checks:

```text
VPS -> 10.8.0.6:5000                     blocked
VPS -> legacy /weather mutation           impossible
VPS -> generic weather proxy path         nonexistent
```

## 9. WireGuard / UFW containment

Existing network:

```text
10.8.0.0/24
10.8.0.1 = old production hub during staging
10.8.0.6 = in-house integration/OpenHAB/weather host
```

### Staging

Verify:

- new VPS has a new WireGuard keypair;
- new VPS uses an inventoried unused temporary `10.8.0.x` address;
- new VPS does not claim `10.8.0.1`;
- old production clients continue using old hub;
- gateway port on `10.8.0.6` is reachable from staging source.

Verify negative paths from staging VPS:

```text
10.8.0.6:5000                    blocked
OpenHAB REST/admin               blocked
trusted-host SSH                 blocked unless explicitly approved
PostgreSQL                       blocked
unrelated LAN hosts              blocked
unrelated WireGuard peers        blocked unless explicitly approved
```

### Production hub cutover rehearsal/config check

Without activating it during staging, inspect/validate the prepared production config where the new VPS will assume:

```text
10.8.0.1/24
```

Verify the documented procedure requires old hub shutdown before activation.

If the VPS is also a WireGuard router/hub, verify routed peer traffic separately from local-process-originated traffic.

## 10. Messaging

Payment event:

- chooses from `sats_received` template pool;
- renders amount/difference correctly;
- produces one logical Nostr event;
- produces overlay event;
- renderer failure does not alter payment credit.

Feeder event:

- chooses from `feeder_trigger` pool;
- publishes only after confirmed physical feed;
- Nostr + overlay presentation succeeds independently of feeder accounting.

Informational events:

- interface/weather visible on overlay;
- never enter Nostr outbox.

## 11. Nostr durable outbox

Test:

- sign failure before outbox persistence;
- relay publish failure;
- restart with pending outbox event;
- successful retry.

Requirement: retry publishes the exact persisted signed event; no new event ID is generated for the same logical message.

## 12. Overlay

Verify:

- snapshot on connect;
- ordered event sequence;
- reconnect/resnapshot;
- gap detection;
- multi-feed backlog;
- payment animation/message;
- feeder animation only on `feeder_confirmed`;
- informational/weather queue;
- client messages cannot cause server-side state changes.

## 13. VPS host hardening

Verify:

- SSH password authentication disabled;
- direct root SSH disabled;
- stale keys/accounts removed;
- production binary/config root-owned;
- runtime user cannot modify binary/config/systemd/nginx;
- deploy user cannot read final runtime secrets after sudo reduction;
- no OpenHAB token exists on VPS;
- no CLN/LNbits secrets remain in production runtime paths;
- service systemd sandbox settings load successfully;
- deployed binary SHA-256 matches recorded release artifact.

## 14. Domain / DNS readiness

Record status of:

- registrar hardware-key MFA;
- DNS-provider hardware-key MFA;
- transfer/domain lock;
- DNSSEC;
- CAA;
- record inventory;
- recovery method.

Controls unsupported by the provider should be marked `N/A` with rationale.

Do not change production DNS as part of staging tests.

## 15. Operational Strike balance

Before production acceptance define and record:

- target online balance;
- maximum online balance;
- manual sweep procedure/destination;
- who performs/reviews the sweep.

Verify staging/production account balance is below the approved maximum before cutover.

## 16. Parallel VPS real canary

Before DNS cutover:

1. use staging hostname/direct SNI/hosts override;
2. verify staging `10.8.0.x` WireGuard peer and gateway-only trusted path;
3. resolve all six configured Lightning Addresses;
4. prove unknown address rejection;
5. pay one tiny real Strike invoice;
6. confirm exactly one durable payment credit/event;
7. verify recipient metadata;
8. verify Nostr + overlay;
9. fetch/weather-render one sanitized overlay-only weather event;
10. confirm direct `10.8.0.6:5000` failure;
11. test harmless/simulated feeder gateway path;
12. with explicit operator approval, perform one controlled physical feed;
13. replay same feed UUID and prove no second actuation.

## 17. Cutover gate

Issue #16 may begin only when:

- all required issue #15 checks pass;
- issues #17–#21 are complete or explicitly waived by the operator with rationale where applicable;
- broad Codex/deploy sudo is revoked/narrowed;
- production secrets/access review is complete;
- old VPS remains recoverable and is still the only active `10.8.0.1` hub;
- final new-VPS `10.8.0.1` hub config is prepared but inactive;
- production DNS remains unchanged until operator starts the cutover runbook.

## F11/F12 corrected presentation acceptance

Use the versioned cursor contract in `../architecture/overlay-stream.md`.
Verify ordered gap replay before its checkpoint, deduplication, explicit resets
for foreign/future/missing/oversized history, and restart-persistent stream IDs.
Exercise actual WebSocket capacity and reuse after disconnect, receive-only input
and frame limits, heartbeat survival beyond the edge timeout, missing Pong and
slow-consumer closure. Repeat against the imported authoritative browser before
closing site compatibility; Rust socket mocks alone do not satisfy that gate.

Weather must reject stale first responses, stale repeats/restarts, future/regressed
observations and malformed timestamps. Check explicit Fahrenheit versus converted
Celsius, optional field units, array ordering, and invalid data not advancing the
watermark. Weather failure or skipped replay must not affect credit/settlement or
enter the Nostr outbox. The earlier optional-timestamp wording is superseded by
mandatory actual-age validation in `../architecture/weather-overlay.md`.

## Actual systemd rehearsal candidate

Base: draft PR #39 head `5becfbd4b8b321ecb205a70233d1970a822645f9`.
The candidate adds actual transient-system-service execution of the canary sandbox
and synthetic encrypted credential delivery. See `../deployment/systemd-rehearsal.md`.
It preserves duplicate/restart command-count checks and adds runtime probes plus
negative wrong-name/corrupt ciphertext failures at systemd CREDENTIALS (243).
The Fedora rehearsal preserves SELinux enforcing and maps only disposable binary
labels to their real installation defaults; no host policy changes are made.
The six added deployment regressions cover credential-store recovery guards and
preservation of repeated/empty sandbox directives. Final local/CI execution
results must be attached before treating this candidate as verified.
