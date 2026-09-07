# Phase 1 Threat and Privilege Model

## Purpose

This document defines the security assumptions for the Strike-backed Lightning Goats migration.

The design goal is not to assume Internet-facing software is perfectly secure. The design goal is to make compromise of one component financially and operationally limited.

## Primary security principle

> Do not rely on hot software being perfectly trustworthy. Minimize the authority and value exposed to each component.

## Protected assets

Highest priority:

- Bitcoin savings/cold storage;
- any spend-capable Strike authority;
- Nostr root/private signing key;
- WireGuard private keys and trusted-network access;
- OpenHAB authority capable of actuating physical devices beyond the intended feeder rule.

Operational assets:

- Strike receive-only credential;
- Strike webhook verification secret;
- Lightning Goats SQLite ledger/event state;
- NIP-46 client credential;
- nginx/TLS configuration;
- feeder control state and `FeederOverride`.

## Assumed hostile inputs

Treat all of these as attacker-controlled:

- HTTP requests to public endpoints;
- LNURL callback parameters;
- webhook bodies before signature and provider-state verification;
- Nostr relay/network traffic;
- malformed BOLT11/provider responses;
- repeated/reordered provider notifications;
- overlay WebSocket reconnect/read behavior;
- unexpected network timeouts and partial failures.

## Trust boundaries

### Public VPS

Assume the VPS may eventually be compromised.

Therefore it must not provide an attacker with:

- a spend-capable Strike credential;
- cold-storage keys;
- a Nostr private key;
- unrestricted home-LAN/WireGuard access;
- CLN/LNbits secrets retained from the old stack.

### Strike

Strike is authoritative for the final state of Strike receive requests/receives, but webhook delivery is not sufficient by itself.

A webhook must be:

1. signature verified;
2. parsed defensively;
3. reconciled against an authoritative Strike API read;
4. committed idempotently.

### OpenHAB / physical feeder

The network result of an actuation request is not proof of physical non-actuation.

If a trigger call has an ambiguous outcome, mark the feed attempt unknown and block automatic retries until operator reconciliation.

### Nostr

Nostr publication is best-effort presentation after durable financial/feeder state commits.

Never make payment credit or feeder accounting dependent on successful Nostr publication.

Use NIP-46/`nak` so the application does not carry the Nostr private signing key.

## Least-privilege credentials

### Strike runtime credential

Phase 1 must have only the permissions required to:

- create receive requests;
- read/reconcile receive requests/receives.

Do not grant outbound payment, withdrawal, bank, conversion, or other spend-capable scopes.

If Phase 2 needs payouts, create a separately privileged worker/credential rather than expanding the Phase 1 daemon by default.

### OpenHAB credential

Prefer a token/account restricted to the exact feeder/status operations required if OpenHAB permits sufficiently granular authorization.

At minimum, network policy from the VPS must restrict reachability to the intended OpenHAB host/port, and application code must only call configured allowlisted item/rule paths.

### Nostr

The daemon receives only the NIP-46 client-side capability needed to request signing. Keep the actual Nostr secret outside the daemon.

## Unix privilege separation

### `lg-deploy` / Codex account

Staging-only broad authority:

- source checkout;
- compiler/toolchain;
- temporary sudo for provisioning.

Before production secrets/cutover:

- remove broad sudo;
- review SSH keys and authorized sessions;
- ensure it cannot read production credential files/directories;
- remove unnecessary package/admin rights.

### `lightning-goats` runtime account

- no sudo;
- no interactive shell if practical;
- no ownership of root-managed system configuration;
- access only to required state directories and systemd-provided credentials.

Do not run Codex under this identity.

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

Review address-family restrictions and filesystem write paths against the final Strike/NIP-46/OpenHAB implementation.

Use systemd credentials (`LoadCredential=` / `LoadCredentialEncrypted=`) or equivalently restricted root-managed secret files. Do not put secrets in Git or world-readable environment files.

## WireGuard containment

The new VPS uses its own WireGuard keypair during staging.

At the trusted/home side, firewall the VPS peer so it can reach only explicitly required internal services. For Phase 1 this should normally be limited to the OpenHAB host/port and any explicitly approved status source.

Do not treat `AllowedIPs` alone as the application authorization boundary; enforce host/forward firewall rules as appropriate.

If the VPS also becomes the WireGuard hub for other clients, distinguish:

- traffic the hub is allowed to route between approved peers; and
- traffic local processes on the VPS are allowed to originate into the trusted network.

Do not accidentally give `lightning-goatsd` or an arbitrary compromised VPS process unrestricted trusted-network reachability.

## Data/idempotency invariants

- provider source IDs are durable unique identifiers;
- payment hashes are unique where available;
- duplicate delivery is idempotent;
- conflicting identity/hash reuse fails closed;
- settlement + credit + payment event is atomic;
- feeder debit + confirmed feeder event is atomic;
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

1. build/configure with no production secrets;
2. run tests/mocks/canary dependencies;
3. finish host provisioning;
4. create production runtime user/system service;
5. revoke deploy/Codex broad sudo;
6. audit ownership/firewall/systemd units;
7. install/rotate final production credentials;
8. final staging verification;
9. operator-authorized cutover.

The point is to avoid a long-lived highly privileged development agent account coexisting unnecessarily with final production secrets.

## Explicit non-goals

Phase 1 does not attempt to make the VPS a cold-storage system or trusted savings wallet.

Phase 1 does not grant Codex autonomous authority to change production DNS, rewire production WireGuard clients, or actuate the real feeder without an operator-directed cutover/test step.
