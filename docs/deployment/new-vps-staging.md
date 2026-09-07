# New VPS Staging Runbook

Status: pre-production procedure for parallel deployment.

Do not use this runbook to perform final DNS/WireGuard cutover. That is covered by `production-cutover.md` and requires explicit operator approval.

## Goal

Build and validate the replacement Lightning Goats stack on a new VPS while the existing VPS remains production-authoritative.

Canonical references:

- `wireguard-topology.md`
- `../architecture/weather-overlay.md`
- `../security/phase1-threat-model.md`
- `../security/phase1-hardening-checklist.md`
- `../security/openhab-feeder-gateway.md`
- `../testing/phase1-verification-matrix.md`

## Existing WireGuard topology

Reuse the existing network:

```text
10.8.0.0/24
```

Known nodes:

```text
10.8.0.1   existing production VPS / WireGuard hub
10.8.0.6   in-house OpenHAB + weather host
```

The new VPS must use a new WireGuard keypair and an unused temporary `10.8.0.x` staging address selected only after inventorying current peers.

Do not assign `10.8.0.1` to the new VPS during staging.

## 1. Provision the VPS

Create a small clean Vultr VPS in the desired region.

Initial host should contain only what is needed for staging:

- SSH;
- package updates;
- WireGuard;
- nginx;
- Rust/build tooling as required by the deploy account;
- Codex under the deploy account;
- repository checkout.

Do not install LNbits, Core Lightning, CLNRest, `clnaddress`, or PostgreSQL for LNbits.

Do not install/copy any OpenHAB API token to this VPS.

## 2. Create account boundaries

Create a deployment account, e.g.:

```text
lg-deploy
```

It may have temporary sudo during staging.

Later create a separate runtime account:

```text
lightning-goats
```

The runtime account should not have sudo and preferably should not allow interactive login.

Do not install Codex under the runtime account.

Every privileged host change should be made reproducible in repo-managed scripts/configuration when practical so the final host can be audited or rebuilt.

## 3. Install Codex and clone the repository

As the deploy account:

- install Codex using the current supported method;
- authenticate it using the operator-approved account flow;
- clone `lightning-goats/lightning-goats`;
- read `AGENTS.md` first;
- read the Phase 1 tracker (#6) and child issues;
- read `docs/README.md` and the canonical planning/architecture/security docs before making changes.

Codex should work issue-by-issue and keep tests/docs updated with each behavioral change.

## 4. Inventory the live WireGuard network

Before assigning the new VPS an internal address, inspect the current hub and relevant peers.

Record:

- every active `10.8.0.x` assignment;
- peer public keys;
- `AllowedIPs`;
- current server listen port/public endpoint;
- forwarding/firewall behavior;
- which clients rely on `10.8.0.1` as hub address.

Useful commands include:

```sh
wg show
ip -br address
```

and review of the actual WireGuard configuration files.

Select one genuinely unused temporary `10.8.0.x` staging address.

## 5. Add the new VPS as a staging peer

Generate a new keypair for the new VPS.

Do not copy/reuse the old VPS private key while both systems are online.

Add the new VPS to the existing `10.8.0.0/24` network using the inventoried unused staging address.

The old VPS remains `10.8.0.1` and production clients continue pointing at it.

Prepare the future production hub configuration for the new VPS separately, but do not activate `10.8.0.1` on the new VPS.

See `wireguard-topology.md`.

## 6. Build the in-house Lightning Goats integration gateway

Implement issue #17 before physical feeder canary testing.

Preferred host is the existing OpenHAB/weather server:

```text
10.8.0.6
```

On the trusted host:

1. create the dedicated OpenHAB `lightning_goats` USER (or approved equivalent);
2. generate a new project-specific API token;
3. store the token only on the gateway host as a systemd credential/root-managed secret;
4. create the dedicated feeder command/ack Items/rule described in `../security/openhab-feeder-gateway.md`;
5. configure local duplicate suppression, `FeederOverride`, remote-enable, minimum physical interval, and absolute safety/feed cap;
6. install the narrow integration-gateway service;
7. bind it only to the intended WireGuard address/interface;
8. include sanitized read-only `/v1/weather` support from issue #21;
9. do not expose a generic OpenHAB or legacy-weather proxy.

The legacy weather receiver currently runs on:

```text
10.8.0.6:5000
```

The gateway should read it locally from `127.0.0.1:5000/get_received_data` when colocated.

## 7. Apply trusted-side UFW/firewall restrictions

During staging, permit the temporary new-VPS `10.8.0.x` source to reach only the dedicated integration-gateway TCP port on `10.8.0.6`.

The staging VPS must **not** directly reach:

```text
10.8.0.6:5000   legacy weather Flask service
10.8.0.6:8080   OpenHAB REST/UI if standard port
10.8.0.6:22     SSH
PostgreSQL
unrelated LAN hosts
unrelated WireGuard peers
```

unless an explicit operator-approved exception is documented.

If the new VPS will later function as the WireGuard hub for existing clients, prepare forwarding/routing rules separately from local-origin application access policy.

Record the exact UFW/nftables/WireGuard rules in repo docs/scripts.

## 8. Verify weather gateway compatibility

Implement issue #21 / `../architecture/weather-overlay.md`.

Confirm:

- gateway can read current local weather state;
- sanitized `/v1/weather` returns typed expected values;
- overlay message format matches existing Lightning Goats behavior;
- weather event is overlay-only;
- configured defaults begin at 60-second evaluation interval / 0.30 broadcast probability unless operator changes them;
- direct VPS `10.8.0.6:5000` fails;
- legacy `/weather` mutation endpoint is not reachable through the gateway.

## 9. Stage nginx and static site

Serve the Lightning Goats static site locally on the new VPS.

Prepare nginx routes for:

- static `lightning-goats.com` content;
- generic Lightning Address discovery path (`/.well-known/lnurlp/<user>`);
- generic LNURL-pay callback path;
- Strike webhook;
- `/healthz`;
- `/api/v1/status`;
- overlay WebSocket;
- any explicitly required canary paths.

Generic nginx routing must not create wildcard Lightning Address behavior. The application registry remains authoritative.

Configure public abuse controls from issue #20.

Do not change production DNS yet.

Use a temporary hostname, hosts-file override, direct IP/SNI test method, or other controlled staging route.

## 10. Implement the configured Lightning Address registry

Implement issue #18 and `../architecture/lightning-address-registry.md`.

Required configured users:

```text
herd
dexter
rowan
cosmo
newton
nova
```

All six must resolve through the same generic LNURL application path, create Strike-backed invoices, map to `credit_pool=herd`, and preserve the actual `address_user` durably.

Unknown users must fail before any Strike API call.

## 11. Implement remaining Phase 1 issues

Use the recommended ordering in `docs/planning/phase1-execution-plan.md`.

Codex must not reintroduce LNbits/CLN, direct generic OpenHAB access, or direct weather-service access as shortcuts.

Run the locked Rust gates after each meaningful implementation slice.

## 12. Prepare system-level production units

Use root-managed systemd units under `/etc/systemd/system/` rather than user-level units for production.

The `lightning-goatsd` unit should run as the `lightning-goats` runtime user and retain strong sandboxing.

The in-house integration gateway should run as its own non-admin service identity/system unit.

Production binary/config should be root-owned and not writable by the runtime or non-privileged deploy user.

Record the final binary SHA-256 and source commit/tag.

## 13. Staging credentials

Final runtime VPS should receive only:

- receive/read-only Strike API credential;
- Strike webhook verification secret;
- NIP-46 client credential/config;
- optional low-value integration-gateway client credential if used.

It must not receive:

- Strike spend/withdraw authority;
- OpenHAB API token;
- old LNbits/CLN secrets.

The dedicated OpenHAB token is installed only on the in-house integration gateway.

If extra Strike scope is needed to create/manage the webhook, use a separate deployment-time key and revoke it after configuration/verification unless the operator explicitly retains it for operations.

## 14. Reduce Codex/deploy privilege before final secrets

Recommended sequence:

1. finish host configuration;
2. capture privileged host changes in reviewed artifacts;
3. run tests using mocks/staging credentials;
4. create/finalize system services;
5. configure WireGuard/UFW/OpenHAB/weather gateway boundary;
6. revoke/narrow broad deploy sudo;
7. audit sudoers, SSH keys, ownership and filesystem permissions;
8. optionally rebuild/reimage from reviewed artifacts for maximum assurance;
9. install/rotate final production runtime credentials in the correct trust domains;
10. run final staging verification.

## 15. SSH/host hardening

Before staging acceptance:

- SSH keys only;
- disable password authentication;
- disable direct root SSH login;
- remove stale keys/accounts;
- after WireGuard is proven, restrict SSH to WireGuard/admin source ranges where operationally practical;
- confirm Vultr console/recovery path remains available.

## 16. Domain/DNS readiness (no cutover yet)

Prepare, but do not switch DNS.

Record/apply where supported:

- hardware-key MFA on registrar/DNS provider;
- registrar transfer/domain lock;
- DNS change protections;
- DNSSEC;
- CAA;
- production DNS record inventory and recovery process.

## 17. Operational Strike balance policy

Before production acceptance, operator defines target online Strike balance, maximum online Strike balance, and manual sweep procedure/destination.

Do not invent these values.

## 18. Verification before canary acceptance

Use `../testing/phase1-verification-matrix.md` and issue #15 as authoritative.

Required checks include:

- nginx configuration/TLS/staging behavior;
- all six configured Lightning Addresses;
- unknown address -> zero Strike provider calls;
- LNURL rate-limit/validation controls;
- real tiny Strike invoice creation/payment;
- webhook verification and exactly-once credit;
- Nostr publication;
- overlay display and reconnect/replay;
- weather overlay message via `/v1/weather`;
- overlay-only informational/weather messages never publish to Nostr;
- feeder gateway positive/negative paths;
- WireGuard/UFW positive gateway and negative `10.8.0.6:5000`/OpenHAB/SSH reachability tests;
- harmless/simulated OpenHAB canary;
- one operator-approved controlled physical feeder test;
- replay same feeder UUID proves no second physical actuation;
- deployed binary hash/provenance;
- host/SSH/account/secret checks.

## 19. Staging completion gate

Staging is accepted only when:

- all Phase 1 code gates are green;
- issues #17–#21 are complete or explicitly operator-waived with rationale where applicable;
- the new VPS passes the end-to-end verification matrix;
- production runtime account/system service is in place;
- integration gateway/OpenHAB/weather boundary is in place;
- broad temporary deploy sudo has been revoked or narrowed;
- production secret/access review is complete;
- production DNS is still unchanged;
- old VPS remains `10.8.0.1` and available as rollback/archive;
- final new-VPS `10.8.0.1` hub configuration is prepared but inactive.

At that point stop and obtain explicit operator approval before executing `docs/deployment/production-cutover.md`.
