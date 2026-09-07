# Production Cutover and Rollback Runbook

Status: operator-gated production procedure.

Tracker: issue #16.

## Preconditions

Do not begin unless:

- issue #15 verification matrix and `../testing/phase1-verification-matrix.md` have passed on the new VPS;
- issues #17–#21 are complete or explicitly operator-waived with rationale where applicable;
- the old VPS remains intact and recoverable;
- the new VPS has a distinct tested WireGuard identity and temporary staging `10.8.0.x` address;
- the reviewed production hub configuration for `10.8.0.1/24` is prepared but inactive;
- the in-house integration-gateway WireGuard/UFW boundary on `10.8.0.6` is tested;
- `lightning-goatsd` has no OpenHAB token and cannot directly reach generic OpenHAB REST/admin or `10.8.0.6:5000`;
- the in-house gateway has its dedicated OpenHAB USER/token and local safety rules;
- sanitized weather `/v1/weather` and overlay message behavior are verified;
- final production systemd units run under non-admin runtime identities;
- broad temporary Codex/deploy sudo has been revoked or narrowed;
- final production credentials are installed and audited in the correct trust domains;
- Strike runtime credential is receive/read-only and cannot spend;
- any temporary/admin Strike webhook-management credential has been revoked/removed unless explicitly retained for operations;
- domain/DNS security controls and recovery ownership have been reviewed;
- deployed binary hash/provenance has been recorded;
- operator-approved operational Strike balance ceiling/sweep policy exists and current balance is within it;
- feeder operator is available to control `LightningGoatsRemoteEnabled` and `FeederOverride`;
- a current archive exists of old LNbits/config/nginx/WireGuard/CLN recovery material.

## Existing WireGuard topology

Production network:

```text
10.8.0.0/24
```

Before cutover:

```text
10.8.0.1   old/current production VPS hub
10.8.0.6   in-house OpenHAB/weather/integration host
10.8.0.X   temporary staging address for new VPS
```

Preferred final state preserves `10.8.0.1` as the hub address, but with the **new VPS keypair/public endpoint**.

See `wireguard-topology.md`.

## Required production Lightning Addresses

The final service configuration must include:

```text
herd@lightning-goats.com
dexter@lightning-goats.com
rowan@lightning-goats.com
cosmo@lightning-goats.com
newton@lightning-goats.com
nova@lightning-goats.com
```

All six map to the same `herd` feed-credit pool while preserving the paid `address_user`.

No arbitrary wildcard user is authorized.

## Cutover principle

The old VPS stays production-authoritative until the final switch.

The new VPS becomes authoritative only after:

1. legacy side effects are stopped;
2. old WireGuard hub is stopped and `10.8.0.1` is free;
3. new VPS assumes the reviewed `10.8.0.1` hub configuration;
4. required clients update to the new hub public key/public endpoint;
5. the integration-gateway path is healthy and tightly contained;
6. DNS points to the new VPS;
7. the new payment path is verified with tiny real payments before physical feeder enablement.

## 1. Freeze old feeder/payment side effects

Set local feeder safety gates so automatic physical feeding is blocked:

```text
LightningGoatsRemoteEnabled = OFF
FeederOverride = ON
```

Stop the old Lightning-Goats/LNbits feeder-side processes that could mutate goat-feeder accounting.

Archive/log the exact cutover timestamp.

Do not delete old databases or configuration.

## 2. Confirm new VPS and trusted gateway readiness

On the new host confirm:

- nginx config valid;
- `lightning-goatsd` healthy;
- Strike receive/read API access valid;
- webhook route reachable;
- Nostr signer/publisher healthy;
- overlay status/WebSocket healthy;
- staging WireGuard peer healthy;
- configured address registry contains all six expected users;
- public abuse/rate-limit configuration is loaded;
- SQLite state is the intended fresh Phase 1 accounting epoch.

On `10.8.0.6` confirm:

- integration gateway healthy;
- dedicated OpenHAB token is present only there;
- request UUID/ack path works;
- local duplicate suppression and physical safety gates are enabled;
- sanitized `/v1/weather` works;
- local weather receiver remains available to the gateway at `127.0.0.1:5000/get_received_data`;
- UFW/firewall rules are ready to transition from temporary staging source to production source `10.8.0.1`.

From the staging VPS repeat negative reachability checks:

```text
gateway port                   reachable
10.8.0.6:5000                 blocked
OpenHAB REST/admin             blocked
trusted SSH                    blocked unless explicitly approved
PostgreSQL                     blocked
unrelated LAN/WG services      blocked
```

## 3. Move WireGuard hub role to the new VPS

This step is operator-gated.

1. Record the old hub's current state/configuration.
2. Stop WireGuard on the old VPS.
3. Verify the old VPS no longer answers as `10.8.0.1`.
4. Remove/disable the new VPS temporary staging interface/address as required by the reviewed configuration.
5. Activate the new VPS production hub configuration using:

```text
10.8.0.1/24
```

6. Update existing clients' hub peer configuration from:

```text
PublicKey = <old-vps-public-key>
Endpoint  = <old-vps-public-ip>:<wireguard-port>
```

to:

```text
PublicKey = <new-vps-public-key>
Endpoint  = <new-vps-public-ip>:<wireguard-port>
```

7. Preserve client private keys and client `10.8.0.x` addresses unless a separately reviewed change is required.
8. Confirm required clients handshake with the new hub.
9. On `10.8.0.6`, replace the temporary staging-source gateway firewall allowance with the final production rule permitting source `10.8.0.1` only to the integration-gateway port.
10. Remove temporary staging firewall/address rules.
11. Repeat positive/negative trusted-side reachability tests from the new production hub.

Never run both old and new hubs as `10.8.0.1` simultaneously.

## 4. Verify integration gateway and weather after hub migration

Before DNS change:

- gateway health works from new `10.8.0.1`;
- `/v1/feeder/override` works;
- same-UUID feeder status/read path works without actuation;
- `/v1/weather` returns sanitized current weather;
- `10.8.0.6:5000` remains directly blocked from VPS;
- legacy `/weather` mutating endpoint is not exposed;
- OpenHAB REST/admin remains directly blocked.

## 5. Activate production nginx configuration

Install/reload the final production virtual hosts/routes on the new VPS.

Verify locally/directly before DNS change:

- static site;
- generic Lightning Address discovery route;
- LNURL callback;
- webhook endpoint;
- overlay WebSocket;
- health/status;
- configured user allowlist behavior;
- unknown user rejection;
- nginx rate limits/body/method restrictions.

## 6. Change DNS

Point required production names to the new VPS, including at least `lightning-goats.com` / `www` and any other still-required public hostname.

Record old and new DNS values and change time.

Monitor resolution from multiple resolvers if practical.

Do not weaken registrar/DNS security controls to make the cutover easier.

## 7. Verify public Lightning Address path

Once DNS resolves to the new VPS:

1. load `https://lightning-goats.com`;
2. resolve `herd@lightning-goats.com` through a real wallet/client;
3. resolve each goat address (`dexter`, `rowan`, `cosmo`, `newton`, `nova`);
4. confirm an intentionally unknown address is rejected and does not create a Strike receive request;
5. request a tiny invoice for one configured address;
6. pay a very small amount;
7. confirm Strike marks it completed;
8. confirm `lightning-goatsd` credits it exactly once;
9. confirm durable settlement/event records preserve the correct `address_user` and `credit_pool=herd`;
10. confirm one durable `payment_received` event;
11. confirm templated Nostr publication;
12. confirm overlay message/animation;
13. verify no feeder actuation occurs while local safety gates remain blocking.

If any financial-state or address-registry invariant fails, stop and investigate before enabling the feeder.

## 8. Verify weather presentation in production

Confirm:

- `lightning-goatsd` can obtain sanitized weather only through gateway `/v1/weather`;
- the overlay receives a correctly formatted `weather_status` message;
- the weather message never enters the Nostr outbox;
- direct `10.8.0.6:5000` access remains blocked;
- weather receiver failure does not affect payment/feeding.

## 9. Controlled feeder activation

After payment ingress/presentation is accepted:

1. arrange a controlled threshold condition;
2. verify the exact pending feed-attempt UUID/state;
3. keep `FeederOverride=ON` / remote enable OFF while checking state;
4. with explicit operator approval, enable the remote feeder path and release the override as required;
5. confirm exactly one gateway request UUID;
6. confirm exactly one physical feeding for one due threshold;
7. confirm authoritative acknowledgement of the same UUID;
8. confirm exactly one feed debit;
9. confirm exactly one durable `feeder_confirmed` event;
10. confirm templated feeder message on Nostr + overlay;
11. replay/query the same UUID and confirm no second physical actuation;
12. restore the desired normal local safety state.

For multiple due feeds, verify serialization, local minimum physical-feed interval, and safety/feed caps.

Any ambiguous gateway/physical outcome remains `unknown`/unresolved and must not trigger a fresh automatic actuation.

## 10. Confirm security/operational state after cutover

Before declaring success verify:

- runtime Strike key remains receive/read only;
- OpenHAB token exists only on the in-house gateway;
- deploy/Codex user cannot read production secrets or modify production binary/config;
- SSH remains key-only with root login disabled;
- deployed binary hash still matches recorded artifact;
- operational Strike balance remains below approved maximum;
- Nostr outbox has no unexpected backlog;
- no unresolved feeder attempt exists;
- DNS/registrar security settings remain intact;
- WireGuard topology matches documented `10.8.0.0/24` final state;
- temporary staging address/UFW rules are removed.

## Rollback boundary

### Easy rollback

Before the new stack has accepted meaningful new-epoch payments, rollback may consist of:

1. keep local feeder remote-enable/override gates blocking;
2. stop WireGuard on the new VPS so it releases `10.8.0.1`;
3. restore old VPS WireGuard `10.8.0.1` hub;
4. repoint clients to old hub public key/endpoint;
5. restore DNS to old VPS;
6. restore only explicitly required legacy public services.

Never have both hubs active as `10.8.0.1`.

Do not automatically reactivate CLN routing capital as part of rollback.

### After new-epoch payments

Once the new Strike-backed system has accepted payments, do not perform a blind rollback that discards its SQLite feed-credit/event state.

Preserve the new SQLite database, account for all accepted payments/feed credit, preserve `address_user`, and reconcile unresolved feed attempts before routing payment traffic elsewhere.

### Integration-gateway rollback

A problem in the integration gateway does not require redirecting payments immediately.

Prefer keeping `LightningGoatsRemoteEnabled=OFF` / `FeederOverride=ON`, pausing or continuing payment ingress according to operator choice, repairing the gateway safely, and never bypassing it with generic OpenHAB/weather access from the VPS.

## Old VPS retirement

After successful cutover:

- power down/disable unnecessary old services;
- retain the old VPS intact for an observation period;
- retain LNbits data read-only for historical audit as desired;
- retain CLN recovery material offline;
- retain old nginx/WireGuard configs in an encrypted/archive location;
- do not destroy the old Vultr instance until backups and new production behavior have been verified.

After the observation period, destroy/downgrade the old VPS to stop billing.

## Final acceptance

Phase 1 is complete when:

- all six configured Lightning Addresses use native LNURL-pay + Strike;
- arbitrary unconfigured users fail closed;
- no LNbits/CLN/clnaddress runtime path serves production payments;
- Strike settlements credit exactly once and preserve recipient metadata;
- the VPS has no OpenHAB token/direct generic OpenHAB or weather-service access;
- weather messages work through the sanitized gateway and remain overlay-only;
- feeder request/ack, duplicate suppression, threshold/remainder/ambiguity behavior is verified;
- payment and feeder messages appear on Nostr + overlay;
- WireGuard final state uses the approved `10.8.0.0/24` topology with new VPS as `10.8.0.1` hub;
- UFW containment matches the approved topology;
- host/domain/deployment hardening checks pass;
- operational Strike balance policy is active;
- old VPS is archival/rollback only;
- tracker #6 and its Phase 1 child issues are complete.
