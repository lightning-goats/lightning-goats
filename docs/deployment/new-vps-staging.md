# New VPS Staging Runbook

Status: pre-production procedure for parallel deployment.

Do not use this runbook to perform final DNS/WireGuard cutover. That is covered by `production-cutover.md` and requires explicit operator approval.

## Goal

Build and validate the replacement Lightning Goats stack on a new VPS while the existing VPS remains production-authoritative.

Canonical security references:

- `../security/phase1-threat-model.md`
- `../security/phase1-hardening-checklist.md`
- `../security/openhab-feeder-gateway.md`
- `../testing/phase1-verification-matrix.md`

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

## 4. Add the new VPS to WireGuard as a new peer

Generate a new keypair for the new VPS.

Do not copy/reuse the old VPS private key while both systems are online.

Add the new VPS to the existing WireGuard network with a staging/new peer identity and suitable WireGuard IP.

The production clients continue pointing at the old VPS during this stage.

### Lightning Goats application path

Prefer a separate point-to-point/application WireGuard interface/key/subnet between the new VPS and the trusted feeder-gateway host, or implement equivalently strict per-peer filtering.

Do not grant the VPS generic home-LAN/OpenHAB reachability merely because it is a WireGuard peer.

## 5. Build the in-house OpenHAB feeder gateway

Implement issue #17 before physical feeder canary testing.

On the trusted OpenHAB host or adjacent in-house host:

1. create the dedicated OpenHAB `lightning_goats` USER (or approved equivalent);
2. generate a new project-specific API token;
3. store the token only on the gateway host as a systemd credential/root-managed secret;
4. create the dedicated feeder command/ack Items/rule described in `../security/openhab-feeder-gateway.md`;
5. configure local duplicate suppression, `FeederOverride`, remote-enable, minimum physical interval, and absolute safety/feed cap;
6. install the narrow feeder-gateway service;
7. bind it only to the intended trusted/WireGuard address;
8. do not expose a generic OpenHAB proxy.

## 6. Apply trusted-side UFW/firewall restrictions

Permit the new VPS peer to reach only Phase 1-required destinations.

Expected application allowlist should normally be equivalent to:

```text
new VPS -> feeder gateway host:gateway port
```

The VPS must **not** directly reach generic OpenHAB REST/admin endpoints.

Verify from the new VPS that these fail unless explicitly approved:

```text
OpenHAB REST/admin port
trusted-side SSH
PostgreSQL
unrelated LAN hosts
unrelated WireGuard peers
```

If the VPS will later function as a WireGuard hub for existing clients, prepare forwarding/routing rules separately from local-origin access policy.

Record the exact UFW/nftables/WireGuard rules in repo docs/scripts.

## 7. Stage nginx and static site

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

Configure public abuse controls from issue #20:

- stricter rate limit for invoice-creating callbacks;
- reasonable discovery limit;
- webhook method/content-type/body-size constraints;
- no public debug/admin mutation routes.

Do not change production DNS yet.

Use a temporary hostname, hosts-file override, direct IP/SNI test method, or other controlled staging route.

## 8. Implement the configured Lightning Address registry

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

All six must:

- resolve through the same generic LNURL application path;
- create Strike-backed invoices;
- map to `credit_pool=herd`;
- preserve the actual `address_user` durably.

Unknown users must fail before any Strike API call.

## 9. Implement remaining Phase 1 issues

Use the recommended ordering in `docs/planning/phase1-execution-plan.md`.

Codex must not reintroduce LNbits/CLN or direct generic OpenHAB access as shortcuts.

Run the locked Rust gates after each meaningful implementation slice.

## 10. Prepare system-level production units

Use root-managed systemd units under `/etc/systemd/system/` rather than user-level units for production.

The `lightning-goatsd` unit should run as the `lightning-goats` runtime user and retain strong sandboxing.

The in-house feeder gateway should run as its own non-admin service identity/system unit.

Production binary/config should be root-owned and not writable by the runtime or non-privileged deploy user.

Runtime state should be writable only where required.

Record the final binary SHA-256 and source commit/tag.

## 11. Staging credentials

Use test/staging credentials where possible.

### VPS credentials

Final runtime VPS should receive only:

- receive/read-only Strike API credential;
- Strike webhook verification secret;
- NIP-46 client credential/config;
- optional low-value feeder-gateway client credential if used.

It must not receive:

- Strike spend/withdraw authority;
- OpenHAB API token;
- old LNbits/CLN secrets.

### Trusted gateway credential

The dedicated OpenHAB token is installed only on the in-house feeder gateway.

### Webhook management

If extra Strike scope is needed to create/manage the webhook, use a separate deployment-time key and revoke it after configuration/verification unless the operator explicitly retains it for operations.

## 12. Reduce Codex/deploy privilege before final secrets

Recommended sequence:

1. finish host configuration;
2. capture privileged host changes in reviewed artifacts;
3. run tests using mocks/staging credentials;
4. create/finalize system services;
5. configure WireGuard/UFW/OpenHAB gateway boundary;
6. revoke/narrow broad deploy sudo;
7. audit sudoers, SSH keys, ownership and filesystem permissions;
8. optionally rebuild/reimage from reviewed artifacts for maximum assurance;
9. install/rotate final production runtime credentials in the correct trust domains;
10. run final staging verification.

## 13. SSH/host hardening

Before staging acceptance:

- SSH keys only;
- disable password authentication;
- disable direct root SSH login;
- remove stale keys/accounts;
- after WireGuard is proven, restrict SSH to WireGuard/admin source ranges where operationally practical;
- confirm Vultr console/recovery path remains available.

## 14. Domain/DNS readiness (no cutover yet)

Prepare, but do not switch DNS.

Record/apply where supported:

- hardware-key MFA on registrar/DNS provider;
- registrar transfer/domain lock;
- DNS change protections;
- DNSSEC;
- CAA;
- production DNS record inventory and recovery process.

These controls are part of issue #19.

## 15. Operational Strike balance policy

Before production acceptance, operator defines:

- target online Strike balance;
- maximum online Strike balance;
- manual sweep procedure/destination.

Do not invent these values. Record operator-approved values in the appropriate private/operational location without exposing sensitive wallet details in public Git if inappropriate.

## 16. Verification before canary acceptance

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
- overlay-only informational messages do not publish to Nostr;
- feeder gateway positive/negative paths;
- WireGuard/UFW negative reachability tests;
- harmless/simulated OpenHAB canary;
- one operator-approved controlled physical feeder test;
- replay same feeder UUID proves no second physical actuation;
- deployed binary hash/provenance;
- host/SSH/account/secret checks.

## 17. Staging completion gate

Staging is accepted only when:

- all Phase 1 code gates are green;
- issues #17–#20 are complete or explicitly operator-waived with rationale;
- the new VPS passes the end-to-end verification matrix;
- production runtime account/system service is in place;
- feeder gateway/OpenHAB integration boundary is in place;
- broad temporary deploy sudo has been revoked or narrowed;
- production secret/access review is complete;
- production DNS is still unchanged;
- old VPS remains available as rollback/archive.

At that point stop and obtain explicit operator approval before executing `docs/deployment/production-cutover.md`.
