# OpenHAB Feeder Gateway Security Design

Status: required Phase 1 architecture.

Tracker: issue #17.

## Purpose

The public VPS and `lightning-goatsd` must not hold a generic OpenHAB API token or have broad access to the in-house OpenHAB server/LAN.

OpenHAB's current authorization model is not sufficiently fine-grained to treat an API token as a per-item/per-rule ACL. Phase 1 therefore enforces least privilege with a dedicated integration identity, a narrow in-house feeder gateway, explicit OpenHAB Items/rules, and WireGuard/UFW containment.

## Trust boundary

```text
Internet
   |
   v
new VPS / lightning-goatsd
   |
   | narrow WireGuard path
   | NO OpenHAB token
   v
in-house feeder gateway
   |
   | localhost/trusted host
   | dedicated OpenHAB USER token
   v
OpenHAB dedicated Items/rule
   |
   v
physical feeder
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

- token is stored only on the in-house feeder gateway host;
- use a restricted systemd credential or root-managed secret file;
- do not copy the token to the VPS;
- do not commit it to Git;
- rotate/revoke the token if the gateway host or deployment process is suspected compromised;
- audit OpenHAB's `Implicit User Role` setting and disable unauthenticated LAN USER access if doing so is compatible with the rest of the house automation deployment.

## Gateway API

The gateway must be a purpose-built API, not a generic OpenHAB reverse proxy.

Recommended minimal surface:

```text
GET  /healthz
GET  /v1/feeder/override
GET  /v1/temperature                 # optional
POST /v1/feeder/request/<uuid>
GET  /v1/feeder/request/<uuid>
```

The gateway should:

- listen only on the intended trusted/WireGuard interface or address;
- accept requests only from the new VPS WireGuard peer;
- impose small request/body/time limits;
- reject unknown paths/methods;
- never expose arbitrary OpenHAB REST paths;
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

## WireGuard and UFW policy

Prefer a dedicated point-to-point or isolated WireGuard interface/subnet for the Lightning Goats application path.

Example only:

```text
VPS WG address:      10.77.77.2
Gateway WG address:  10.77.77.1
Gateway TCP port:    8789
OpenHAB REST:        8080 (not reachable from VPS)
```

Illustrative UFW allow rule on the trusted host:

```sh
ufw allow in on wg-lg from 10.77.77.2 to 10.77.77.1 port 8789 proto tcp comment 'Lightning Goats feeder gateway'
```

Assume default-deny incoming/forwarding for anything not explicitly allowed.

Required negative tests from the VPS:

```text
gateway:8789                 reachable
OpenHAB:8080                 blocked
trusted-host SSH             blocked unless explicitly approved
PostgreSQL                   blocked
unrelated LAN hosts/ports    blocked
```

Do not rely on WireGuard `AllowedIPs` alone as authorization. Enforce host/forward firewall rules.

## Service identity and systemd

Run the gateway as its own unprivileged in-house service identity, not as the OpenHAB OS account when practical.

Use a system-level systemd unit with:

- `NoNewPrivileges=yes`;
- filesystem and capability restrictions appropriate to the implementation;
- credential injection for the OpenHAB token;
- explicit listen address;
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
- no generic OpenHAB path access through the gateway.

Staging tests:

- UFW/WireGuard positive and negative connectivity;
- harmless canary/simulated feed request;
- one controlled physical feeder test;
- replay same UUID and prove no second physical actuation.

## Phase 2 compatibility

CyberHerd must not bypass this gateway. Any future feeder-triggering producer should emit durable credit/events through the same Lightning Goats feeder authority rather than gaining direct OpenHAB credentials.