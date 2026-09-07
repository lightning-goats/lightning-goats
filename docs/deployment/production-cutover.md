# Production Cutover and Rollback Runbook

Status: operator-gated production procedure.

Tracker: issue #16.

## Preconditions

Do not begin unless:

- issue #15 verification matrix and `../testing/phase1-verification-matrix.md` have passed on the new VPS;
- issues #17–#20 are complete or explicitly operator-waived with rationale;
- the old VPS remains intact and recoverable;
- the new VPS has a distinct tested WireGuard identity;
- the dedicated Lightning Goats feeder-gateway WireGuard/UFW boundary is tested;
- `lightning-goatsd` has no OpenHAB token and cannot directly reach generic OpenHAB REST/admin endpoints;
- the in-house feeder gateway has its dedicated OpenHAB USER/token and local safety rules;
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
2. required WireGuard clients are repointed;
3. the feeder-gateway application path is healthy and tightly contained;
4. DNS points to the new VPS;
5. the new payment path is verified with tiny real payments before physical feeder enablement.

## 1. Freeze old feeder/payment side effects

Set local feeder safety gates so automatic physical feeding is blocked:

```text
LightningGoatsRemoteEnabled = OFF (recommended during cutover)
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
- WireGuard healthy;
- configured address registry contains all six expected users;
- public abuse/rate-limit configuration is loaded;
- SQLite state is the intended fresh Phase 1 accounting epoch.

On the trusted/home side confirm:

- feeder gateway healthy;
- dedicated OpenHAB token is present only there;
- request UUID/ack path works;
- local duplicate suppression and physical safety gates are enabled;
- UFW/firewall rules allow only the expected VPS -> gateway path.

From the VPS repeat negative reachability checks:

```text
gateway port                  reachable
OpenHAB REST/admin            blocked
trusted SSH                   blocked unless explicitly approved
PostgreSQL                    blocked
unrelated LAN/WG services     blocked
```

## 3. Switch WireGuard hub/client peers

If existing clients currently use the old VPS as their WireGuard server/hub, update their server peer entry from the old VPS to the new VPS.

Normally this means changing:

```text
PublicKey = <old-vps-public-key>
Endpoint  = <old-vps-public-ip>:<wireguard-port>
```

to:

```text
PublicKey = <new-vps-public-key>
Endpoint  = <new-vps-public-ip>:<wireguard-port>
```

Preserve client private keys and client WireGuard addresses unless the approved topology says otherwise.

Confirm each required client handshakes with the new VPS and can reach only intended peers/services.

The dedicated/narrow feeder application tunnel remains independently constrained even if the VPS is also the general WireGuard hub.

If the operator instead decides to reuse the old VPS WireGuard identity, first stop WireGuard on the old VPS and verify it cannot answer before activating that identity on the new host. Never run one peer private key on both hosts simultaneously.

## 4. Activate production nginx configuration

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

## 5. Change DNS

Point required production names to the new VPS, including at least `lightning-goats.com` / `www` and any other still-required public hostname.

Record old and new DNS values and change time.

Monitor resolution from multiple resolvers if practical.

Do not weaken registrar/DNS security controls to make the cutover easier.

## 6. Verify public Lightning Address path

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

## 7. Controlled feeder activation

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

## 8. Confirm security/operational state after cutover

Before declaring success verify:

- runtime Strike key remains receive/read only;
- OpenHAB token exists only on the in-house gateway;
- deploy/Codex user cannot read production secrets or modify production binary/config;
- SSH remains key-only with root login disabled;
- deployed binary hash still matches recorded artifact;
- operational Strike balance remains below approved maximum;
- Nostr outbox has no unexpected backlog;
- no unresolved feeder attempt exists;
- DNS/registrar security settings remain intact.

## Rollback boundary

### Easy rollback

Before the new stack has accepted meaningful new-epoch payments, rollback may consist of:

- restore DNS to old VPS;
- repoint WireGuard clients to old VPS peer;
- keep local feeder remote-enable/override gates blocking;
- restore only explicitly required legacy public services.

Do not automatically reactivate CLN routing capital as part of rollback.

### After new-epoch payments

Once the new Strike-backed system has accepted payments, do not perform a blind rollback that discards its SQLite feed-credit/event state.

Before routing payment traffic back elsewhere:

- preserve the new SQLite database;
- account for all accepted payments and resulting feed credit;
- preserve `address_user` metadata;
- reconcile any unresolved feed attempt;
- document how those credits remain authoritative.

The new Phase 1 ledger is authoritative for payments accepted after its production cutover.

### Feeder-gateway rollback

A problem in the feeder gateway does not require redirecting payments immediately.

Prefer:

- keep `LightningGoatsRemoteEnabled=OFF` / `FeederOverride=ON`;
- continue or pause payment ingress according to operator choice;
- repair/reconcile the gateway safely;
- never bypass the gateway by restoring generic OpenHAB access from the VPS.

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
- the VPS has no OpenHAB token/direct generic OpenHAB access;
- feeder request/ack, duplicate suppression, threshold/remainder/ambiguity behavior is verified;
- payment and feeder messages appear on Nostr + overlay;
- informational messages are overlay-only;
- WireGuard/UFW containment matches the approved topology;
- host/domain/deployment hardening checks pass;
- operational Strike balance policy is active;
- old VPS is archival/rollback only;
- tracker #6 and its Phase 1 child issues are complete.
