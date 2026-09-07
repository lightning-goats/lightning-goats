# Phase 1 Threat and Privilege Model

## Purpose

This document defines the security assumptions for the Strike-backed Lightning Goats migration.

The design goal is not to assume Internet-facing software is perfectly secure. The design goal is to make compromise of one component financially and operationally limited.

Companion checklist: `phase1-hardening-checklist.md`.

## Primary security principle

> Do not rely on hot software being perfectly trustworthy. Minimize the authority, reach, and value exposed to each component.

## Protected assets

Highest priority:

- Bitcoin savings/cold storage;
- any spend-capable Strike authority;
- Nostr root/private signing key;
- WireGuard private keys and trusted-network access;
- OpenHAB authority capable of actuating physical devices beyond the intended feeder contract;
- domain/DNS control for `lightning-goats.com`, because it can redirect Lightning Address discovery.

Operational assets:

- Strike receive-only credential;
- Strike webhook verification secret;
- Lightning Goats SQLite ledger/event state;
- NIP-46 client credential;
- nginx/TLS configuration;
- feeder request/ack state and local safety gates;
- dedicated OpenHAB integration credential on the in-house gateway host;
- deployment/release provenance.

## Assumed hostile inputs

Treat all of these as attacker-controlled:

- HTTP requests to public endpoints;
- Lightning Address usernames and LNURL callback parameters;
- webhook bodies before signature and provider-state verification;
- Nostr relay/network traffic;
- malformed BOLT11/provider responses;
- repeated/reordered provider notifications;
- overlay WebSocket reconnect/read behavior;
- feeder-gateway requests arriving from a compromised VPS;
- unexpected network timeouts and partial failures;
- potentially compromised dependencies/build tooling until verified.

## Trust boundaries

### Public VPS

Assume the VPS may eventually be compromised.

Therefore it must not provide an attacker with:

- a spend-capable Strike credential;
- cold-storage keys;
- a Nostr private key;
- an OpenHAB API token;
- direct generic OpenHAB REST/admin access;
- unrestricted home-LAN/WireGuard access;
- CLN/LNbits secrets retained from the old stack.

The VPS may possess only the operational capabilities necessary to receive payments, publish presentation events through NIP-46, and submit narrowly constrained feeder requests to the in-house gateway.

### Domain / DNS

Control of `lightning-goats.com` is part of the payment trust boundary.

An attacker who can modify DNS or obtain unauthorized certificates can redirect Lightning Address discovery/callbacks or impersonate the public service.

Therefore registrar/DNS accounts require strong MFA/locks/recovery controls, with DNSSEC/CAA where supported and appropriate.

### Strike

Strike is authoritative for the final state of Strike receive requests/receives, but webhook delivery is not sufficient by itself.

A webhook must be:

1. constrained to the expected HTTP route/method/body shape;
2. signature verified;
3. parsed defensively;
4. reconciled against an authoritative Strike API read;
5. committed idempotently.

The runtime key must remain receive/read only.

If webhook setup requires additional scope, use a separate temporary/admin key and revoke it after use rather than expanding runtime authority.

### Lightning Address registry

Generic nginx routing does not authorize arbitrary users.

Only configured users may cause provider invoice creation. Phase 1 configured users are:

```text
herd
dexter
rowan
cosmo
newton
nova
```

Unknown/invalid users and invalid amounts must fail before any Strike request.

All six use the same feeder credit pool while preserving `address_user` metadata.

### In-house feeder gateway / OpenHAB / physical feeder

The public VPS does not hold an OpenHAB credential.

A purpose-built feeder gateway on the trusted side owns the dedicated OpenHAB USER token and exposes only the feeder/status contract documented in `openhab-feeder-gateway.md`.

The gateway/OpenHAB rule is the final physical safety authority and must enforce:

- dedicated remote-enable gate;
- `FeederOverride`;
- duplicate request UUID suppression;
- minimum physical-feed interval;
- configured absolute safety/feed cap;
- hardware/rule prerequisites.

The network result of an actuation request is not proof of physical non-actuation.

If a request may have reached the gateway/OpenHAB but its result is ambiguous, the daemon must preserve the same request UUID for authoritative lookup or mark the feed attempt unknown and block automatic retries. It must never create a fresh automatic actuation just because an HTTP response was lost.

### WireGuard / trusted network

WireGuard encryption is not sufficient authorization by itself.

The trusted side must firewall the Lightning Goats VPS peer to the exact feeder-gateway host/port and any other explicitly approved service.

Direct generic OpenHAB REST/admin access, Postgres, internal SSH, and unrelated LAN/WireGuard hosts must be unavailable from the VPS application path.

Prefer a dedicated application WireGuard interface/key/subnet for VPS -> feeder gateway traffic.

If the VPS also acts as a WireGuard hub for other clients, distinguish routed peer traffic from connections originated by local VPS processes.

### Nostr

Nostr publication is best-effort presentation after durable financial/feeder state commits.

Never make payment credit or feeder accounting dependent on successful Nostr publication.

Use NIP-46/`nak` so the application does not carry the Nostr private signing key.

### Codex / deploy account

Temporary staging sudo is a provisioning capability, not a production trust boundary.

Because code/host changes made while Codex has root can persist after sudo is revoked:

- capture privileged changes in reviewed/reproducible repo assets where practical;
- revoke/narrow sudo before final secrets;
- audit privileged files, sudoers, SSH keys and ownership;
- optionally rebuild/reimage from reviewed artifacts for maximum assurance;
- only then install/rotate final production secrets.

## Least-privilege credentials

### Strike runtime credential

Phase 1 must have only the permissions required to:

- create receive requests;
- read/reconcile receive requests/receives.

Do not grant outbound payment, withdrawal, bank, conversion, or other spend-capable scopes.

If Phase 2 needs payouts, create a separately privileged worker/credential rather than expanding the Phase 1 daemon by default.

### Strike webhook-management credential

If required for setup, it is a deployment-time credential separate from the runtime key.

Revoke/remove it after the production webhook is configured and verified unless continued management is operationally necessary.

### OpenHAB credential

The OpenHAB token belongs only to the in-house feeder gateway.

`lightning-goatsd` receives no OpenHAB credential.

The OpenHAB integration uses a dedicated project USER/token and a strict allowlisted gateway implementation. Network policy prevents the VPS from reaching generic OpenHAB endpoints.

### Nostr

The daemon receives only the NIP-46 client-side capability needed to request signing. Keep the actual Nostr secret outside the daemon.

## Operational value-at-risk

The Strike account remains an online custodial balance even though the runtime API credential cannot spend.

Define an operator-approved:

- target operational balance;
- maximum operational balance;
- manual sweep procedure/destination.

Do not allow goat-feeder receipts to accumulate indefinitely merely because application compromise cannot spend through the API token.

## Unix privilege separation

### `lg-deploy` / Codex account

Staging-only broad authority:

- source checkout;
- compiler/toolchain;
- temporary sudo for provisioning.

Before production secrets/cutover:

- remove/narrow broad sudo;
- review SSH keys and authorized sessions;
- ensure it cannot read production credential files/directories;
- ensure it cannot modify installed production binary/config;
- remove unnecessary package/admin rights.

### `lightning-goats` runtime account

- no sudo;
- no interactive shell if practical;
- no ownership of root-managed system configuration/binaries;
- access only to required state directories and systemd-provided credentials;
- no OpenHAB token.

Do not run Codex under this identity.

### In-house feeder-gateway runtime account

- separate unprivileged service identity;
- no sudo;
- owns only required gateway state;
- receives only dedicated OpenHAB token and optional gateway auth material;
- no Strike or Nostr credential.

## systemd hardening baseline

Retain or improve the current system-service sandboxing where compatible:

```ini
NoNewPrivileges=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectSystem=strict
ProtectHome=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
ProtectKernelLogs=yes
ProtectProc=invisible
ProcSubset=pid
RestrictSUIDSGID=yes
RestrictRealtime=yes
LockPersonality=yes
MemoryDenyWriteExecute=yes
CapabilityBoundingSet=
AmbientCapabilities=
```

Review address-family restrictions and filesystem write paths against the final Strike/NIP-46/gateway implementation.

Use systemd credentials (`LoadCredential=` / `LoadCredentialEncrypted=`) or equivalently restricted root-managed secret files. Do not put secrets in Git or world-readable environment files.

## Public endpoint hardening

### LNURL

- explicit configured user registry;
- strict canonical usernames;
- invalid/unknown users fail before provider contact;
- amount validation before provider contact;
- nginx rate limiting;
- application-level provider-call backpressure;
- no arbitrary template/Nostr/accounting metadata from the requester.

### Strike webhook

- one exact path;
- POST only;
- expected content type;
- small body-size limit;
- constant-time signature comparison;
- authoritative provider read before credit;
- replay/idempotency protection;
- no secret/body/header logging that could expose credentials.

### Health/status/overlay

Expose only the information required for operation/presentation. Avoid public debug/admin mutation endpoints.

Overlay client traffic must remain read-only with respect to server state.

## SSH / host boundary

Before production:

- key-only SSH;
- password authentication disabled;
- direct root login disabled;
- stale keys/accounts removed;
- restrict SSH to WireGuard/admin sources where practical after staging;
- retain provider console for emergency recovery.

## Domain / DNS / TLS hardening

Before cutover record/apply where supported:

- hardware-key MFA for registrar/DNS provider;
- transfer/domain lock;
- DNS change protection;
- DNSSEC;
- CAA;
- production DNS record inventory;
- recovery ownership/process.

## Software supply chain / deployment integrity

- committed `Cargo.lock`;
- locked builds/tests;
- minimal reviewed dependencies;
- security/audit gates;
- pinned/reviewed CI actions;
- release from known commit;
- verify release checksums;
- record deployed binary SHA-256 and source commit/tag;
- production binary/config root-owned and non-writable by runtime/deploy accounts after privilege reduction.

## Data/idempotency invariants

- provider source IDs are durable unique identifiers;
- payment hashes are unique where available;
- `address_user` is durable context;
- configured Phase 1 addresses map to the same `herd` credit pool;
- duplicate delivery is idempotent;
- conflicting identity/hash reuse fails closed;
- settlement + credit + payment event is atomic;
- feeder debit + confirmed feeder event is atomic;
- feed request UUIDs are duplicate-safe at the local physical boundary;
- presentation retries never repeat financial or feeder actions.

## Template/message safety

Only server-known event kinds determine publication audience.

- payment -> Nostr + overlay;
- feeder confirmation -> Nostr + overlay;
- informational/interface/weather -> overlay only.

Template rendering supports simple named fields only. No attribute/index/evaluation syntax.

Malformed templates must degrade presentation only and must not alter financial or feeder state.

## Deployment-time secret sequence

Preferred sequence:

1. build/configure with no final production secrets;
2. run tests/mocks/canary dependencies;
3. finish host provisioning and firewall/gateway setup;
4. create production runtime users/system services;
5. capture/review privileged deployment artifacts;
6. revoke/narrow deploy/Codex broad sudo;
7. audit ownership/firewall/systemd/SSH;
8. install/rotate final Strike/NIP-46/gateway/OpenHAB-side credentials in their correct trust domains;
9. final staging verification;
10. operator-authorized cutover.

## Explicit non-goals

Phase 1 does not attempt to make the VPS a cold-storage system or trusted savings wallet.

Phase 1 does not grant Codex autonomous authority to change production DNS, rewire production WireGuard clients, alter the operator's online-balance policy, or actuate the real feeder without an operator-directed test/cutover step.
