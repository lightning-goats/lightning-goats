# OpenHAB Feeder Gateway Security Design

Status: required Phase 1 architecture.

Tracker: issue #17.

## Purpose

The public VPS and `lightning-goatsd` must not hold a generic OpenHAB API token or have broad access to the in-house OpenHAB server/LAN.

OpenHAB's current authorization model is not sufficiently fine-grained to treat an API token as a per-item/per-rule ACL. Phase 1 therefore enforces least privilege with a dedicated integration identity, a narrow in-house integration gateway, explicit OpenHAB Items/rules, and WireGuard/UFW containment.

## Existing network

The established WireGuard subnet is:

```text
10.8.0.0/24
```

Relevant production addresses:

```text
10.8.0.1   current/old VPS WireGuard hub
10.8.0.6   in-house OpenHAB + weather host
```

During staging, the new VPS receives its own new WireGuard keypair and an unused temporary `10.8.0.x` address. It must not claim `10.8.0.1` while the old VPS remains active.

See `docs/deployment/wireguard-topology.md`.

## Trust boundary

```text
Internet
   |
   v
new VPS / lightning-goatsd
   |
   | existing 10.8.0.0/24 WireGuard network
   | narrowly firewalled application path
   | NO OpenHAB token
   v
10.8.0.6 in-house Lightning Goats integration gateway
   |
   +-- dedicated OpenHAB USER token -> dedicated Items/rule
   +-- trusted local weather read -> 127.0.0.1:5000/get_received_data
   |
   v
physical feeder / sanitized status data
```

A complete compromise of the VPS should provide no generic OpenHAB credential and no general LAN reachability.

## Dedicated OpenHAB identity

Create a dedicated OpenHAB USER account for this project, for example:

```text
lightning_goats
```

Create a dedicated API token associated with that account, for example:

```text
lightning-goats-gateway
```

Requirements:

- token is stored only on the in-house integration gateway host;
- use a restricted systemd credential or root-managed secret file;
- do not copy the token to the VPS;
- do not commit it to Git;
- rotate/revoke the token if the gateway host or deployment process is suspected compromised;
- audit OpenHAB's `Implicit User Role` setting and disable unauthenticated LAN USER access if doing so is compatible with the rest of the house automation deployment.

## Gateway API

The gateway must be a purpose-built API, not a generic OpenHAB or weather-service reverse proxy.

Recommended minimal surface:

```text
GET  /healthz
GET  /v1/feeder/override
GET  /v1/temperature                 # optional
POST /v1/feeder/request/<uuid>
GET  /v1/feeder/request/<uuid>
GET  /v1/weather                     # sanitized/read-only
```

The gateway should:

- listen only on the intended WireGuard address/interface;
- accept requests only from the approved VPS WireGuard source;
- impose small request/body/time limits;
- reject unknown paths/methods;
- never expose arbitrary OpenHAB REST paths;
- never proxy the legacy weather `/weather` mutation endpoint;
- disable HTTP proxy discovery for credential-bearing OpenHAB requests;
- log request IDs and outcomes without logging the OpenHAB token.

If practical, authenticate VPS-to-gateway requests in addition to WireGuard source identity with a separate low-value gateway secret or mTLS. This is defense in depth; the WireGuard/UFW boundary remains mandatory.

## Command / acknowledgement protocol

Prefer dedicated OpenHAB Items over remote rule execution.

Recommended Items:

```text
LightningGoatsFeederRequest   String
LightningGoatsFeederAck       String
LightningGoatsRemoteEnabled   Switch
FeederOverride                existing
AmbientTemperature            existing/optional
```

The `lightning-goatsd` durable feed-attempt UUID becomes the request ID.

### Request flow

1. `lightning-goatsd` durably commits a feed intent with UUID `X`.
2. It sends `POST /v1/feeder/request/X` to the in-house gateway.
3. Gateway validates request shape and submits `X` to `LightningGoatsFeederRequest`.
4. OpenHAB rule evaluates all local safety conditions.
5. If it performs the physical actuation and knows the outcome, it records `LightningGoatsFeederAck = X` (or equivalent structured status).
6. Gateway exposes that status through `GET /v1/feeder/request/X`.
7. `lightning-goatsd` marks the feed confirmed only after authoritative gateway/OpenHAB acknowledgement.

### Local OpenHAB safety gates

The rule must fail closed unless all required conditions pass:

- `LightningGoatsRemoteEnabled == ON`;
- `FeederOverride == OFF`;
- request UUID is valid and has not already been processed;
- no unresolved/local feeder state prevents safe operation;
- minimum physical-feed interval has elapsed;
- configured absolute feed-frequency/safety cap is not exceeded;
- feeder hardware/rule prerequisites are healthy.

The OpenHAB rule is the final authority on whether physical actuation is allowed.

## Exactly-once / ambiguity behavior

Duplicate UUIDs must never actuate twice.

If the gateway or network returns an ambiguous result after the request may have reached OpenHAB:

- `lightning-goatsd` marks the attempt `unknown`;
- it does not automatically retry physical actuation;
- operator reconciliation remains required unless the gateway can later return an authoritative acknowledgement for the same UUID.

If a response is lost after a successful feed, querying the same UUID must reveal the prior outcome rather than causing another actuation.

## Weather read path

The legacy weather service runs on the same in-house host:

```text
10.8.0.6:5000
```

It exposes both:

```text
GET /get_received_data   # read
GET /weather?...         # mutating ingestion
```

Therefore the VPS must not receive direct access to port 5000.

The integration gateway should read locally/trusted-side:

```text
http://127.0.0.1:5000/get_received_data
```

validate/sanitize the expected fields, and expose only:

```text
GET /v1/weather
```

See `docs/architecture/weather-overlay.md` and issue #21.

Weather failures affect presentation only and must never affect payment or feeder accounting.

## WireGuard and UFW policy

Reuse the existing `10.8.0.0/24` WireGuard network.

### During staging

- old production hub remains `10.8.0.1`;
- new VPS uses an inventoried unused temporary `10.8.0.x` address/new keypair;
- UFW on `10.8.0.6` allows that temporary source only to the dedicated gateway TCP port;
- direct access from the staging VPS to `10.8.0.6:5000`, OpenHAB REST, SSH and unrelated services stays blocked.

### At production cutover

After the old VPS WireGuard service is stopped, the new VPS may assume `10.8.0.1` to preserve the established hub topology. Replace/remove the temporary staging UFW rule and allow the final production hub source only to the gateway port.

Illustrative policy shape on `10.8.0.6`:

```text
new/staging VPS source -> gateway port     ALLOW during staging only
10.8.0.1 -> gateway port                  ALLOW in production
VPS -> 10.8.0.6:5000                      BLOCK
VPS -> OpenHAB REST/admin                  BLOCK
VPS -> SSH/Postgres/unrelated services     BLOCK
```

Do not rely on WireGuard `AllowedIPs` alone as authorization. Enforce host/forward firewall rules.

Required negative tests from the VPS include:

```text
10.8.0.6:5000              blocked
OpenHAB REST/admin          blocked
trusted-host SSH            blocked unless explicitly approved
PostgreSQL                  blocked
unrelated LAN hosts/ports   blocked
```

## Service identity and systemd

Run the gateway as its own unprivileged in-house service identity, not as the OpenHAB OS account when practical.

Use a system-level systemd unit with:

- `NoNewPrivileges=yes`;
- filesystem and capability restrictions appropriate to the implementation;
- credential injection for the OpenHAB token;
- explicit WireGuard listen address;
- no write access outside required runtime/state paths.

## Required tests

Automated/unit tests:

- valid/invalid UUID parsing;
- duplicate request suppression;
- override ON/OFF/invalid behavior;
- remote-enable OFF behavior;
- local minimum interval enforcement;
- safety cap enforcement;
- gateway timeout / ambiguous state;
- authoritative later ack for the same UUID;
- no generic OpenHAB path access through the gateway;
- weather normalization/read-only behavior;
- no route to legacy `/weather` mutation.

Staging tests:

- UFW/WireGuard positive and negative connectivity;
- sanitized weather read through `/v1/weather`;
- direct `10.8.0.6:5000` failure from VPS;
- harmless canary/simulated feed request;
- one controlled physical feeder test;
- replay same UUID and prove no second physical actuation.

## Phase 2 compatibility

CyberHerd must not bypass this gateway. Any future feeder-triggering producer should emit durable credit/events through the same Lightning Goats feeder authority rather than gaining direct OpenHAB credentials.
