# Phase 1 Security Hardening Checklist

Status: required before production cutover.

Related issues: #14, #17, #19, #20.

## Security objective

Assume public-facing software and dependencies may contain unknown vulnerabilities. Reduce the authority, network reach, and financial value exposed to each compromised component.

## 1. Strike credentials

### Runtime key

`lightning-goatsd` production credential must be limited to the minimum receive/read operations required by the final implementation.

It must not have:

- outbound Lightning payment authority;
- withdrawal authority;
- bank-management authority;
- exchange/conversion authority unless explicitly required and approved;
- webhook-management/admin scope unless runtime management is actually required.

### Webhook-management key

If webhook creation/management requires additional scope:

1. create a separate temporary/admin key;
2. configure the production webhook;
3. verify it;
4. revoke/delete the management key when no longer needed.

Do not expand the runtime key merely for deployment convenience.

### Operational balance ceiling

Treat the Strike account as an online operational balance, not savings storage.

Before production define:

```text
TARGET_OPERATIONAL_BALANCE_SATS = operator decision
MAX_OPERATIONAL_BALANCE_SATS    = operator decision
SWEEP_DESTINATION               = operator-approved storage
```

Document how often/when excess is swept manually. Review the ceiling during staging/cutover and periodically thereafter.

## 2. OpenHAB/physical feeder

Follow `openhab-feeder-gateway.md`.

Required properties:

- no OpenHAB token on the VPS;
- dedicated OpenHAB USER + project API token only on trusted gateway host;
- narrow feeder gateway, never generic OpenHAB proxy;
- request UUID/ack protocol;
- duplicate UUID suppression locally;
- `FeederOverride` local authority;
- `LightningGoatsRemoteEnabled` local kill switch;
- minimum physical-feed interval enforced locally;
- absolute feed-frequency/safety cap enforced locally;
- ambiguous outcomes never blindly retried.

## 3. WireGuard and trusted-network containment

Prefer a dedicated Lightning Goats application tunnel/interface/subnet for VPS -> home feeder traffic.

At minimum enforce per-peer host/forward firewall rules so the VPS can reach only explicitly required services.

Required negative tests:

- direct OpenHAB REST/admin blocked;
- in-house SSH blocked unless explicitly approved;
- Postgres blocked;
- unrelated LAN/WireGuard hosts blocked;
- feeder gateway reachable only on its expected port.

Do not rely on `AllowedIPs` alone.

If the VPS also routes other WireGuard clients, keep forwarding policy separate from local-process egress policy.

## 4. Public HTTP ingress

### Lightning Address/LNURL

- generic nginx route is acceptable;
- application uses explicit configured user registry;
- unknown users fail before provider contact;
- invalid amounts fail before provider contact;
- callback/invoice creation rate-limited in nginx;
- callback/invoice creation rate-limited/backpressured in application;
- safe response/error sizes;
- no user-controlled template/event/accounting authority.

### Strike webhook

- one exact path;
- POST only;
- expected content type only;
- small maximum request body;
- signature verified using constant-time comparison before side effects;
- webhook notification never treated as authoritative financial state;
- authoritative Strike read before credit;
- duplicate/replay idempotency;
- safe logging without secret/header leakage.

### Other endpoints

- health/status endpoints disclose only operationally necessary data;
- no public debug/admin endpoints;
- overlay WebSocket read-only from client perspective;
- enforce reasonable connection/request limits.

## 5. Domain, DNS, and certificate control

Treat control of `lightning-goats.com` as payment-routing authority because DNS can redirect Lightning Address discovery.

Before production:

- hardware-key MFA on registrar/DNS account where supported;
- registrar transfer lock/domain lock;
- DNS change protection/approval features where supported;
- DNSSEC enabled if the provider/registrar combination supports it reliably;
- CAA records restricting certificate issuance to intended CA(s) where practical;
- inventory all production A/AAAA/CNAME/MX/TXT/CAA records;
- document registrar/DNS recovery method and account ownership;
- remove stale public records from old services where safe.

## 6. VPS SSH and host access

- keys only;
- `PasswordAuthentication no`;
- `PermitRootLogin no` (or equivalent no direct root SSH);
- remove stale SSH keys/accounts;
- restrict SSH by firewall to admin source/WireGuard where practical after staging;
- retain Vultr console as emergency recovery path;
- ensure `lg-deploy` and runtime account are separate.

## 7. Codex / deployment privilege boundary

During staging Codex may have temporary sudo through the deploy account.

Every privileged change should be captured in reviewed/reproducible repo assets where practical:

- provisioning scripts;
- systemd units;
- nginx config;
- firewall/WireGuard config templates/runbooks;
- package list/configuration.

Before final production credentials:

1. revoke/narrow broad sudo;
2. audit sudoers, SSH keys and unexpected privileged files;
3. verify deploy account cannot read runtime secrets;
4. verify runtime account cannot alter binaries/config;
5. optionally rebuild/reimage the VPS from reviewed artifacts for maximum assurance;
6. install/rotate final production secrets only after the boundary is accepted.

## 8. Production runtime filesystem ownership

Recommended ownership model:

```text
/usr/local/bin/lightning-goatsd       root:root, not runtime-writable
/etc/lightning-goats/*                 root:root, restricted
/etc/systemd/system/*.service          root:root
runtime SQLite/state                   lightning-goats:lightning-goats, mode 0700 path
systemd credentials                    root-managed
```

The deploy/Codex account should not be able to modify production binaries/config after privilege reduction.

## 9. systemd sandbox

Retain strong system-service restrictions compatible with the final runtime:

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

Add explicit write-path allowances only where needed.

Review whether additional controls are compatible:

- `ProtectClock=yes`;
- `ProtectHostname=yes`;
- `ProtectKernelLogs=yes`;
- `SystemCallArchitectures=native`;
- `RestrictNamespaces=yes` where compatible;
- `IPAddressDeny=`/`IPAddressAllow=` or nftables policy where maintainable;
- resource limits/restart throttling.

Do not enable a hardening directive blindly if it breaks required Rust/SQLite/network/NIP-46 behavior; test it.

## 10. Software supply chain and deployment provenance

- keep `Cargo.lock` committed;
- use `--locked` CI/builds;
- run format/clippy/tests/security audit gates;
- minimize new dependencies and prefer small HTTP adapters where practical;
- pin/review GitHub Actions dependencies;
- release/build from a known commit;
- record release tag/commit and SHA-256 of deployed binaries;
- verify downloaded release checksums before install;
- document toolchain version used for release builds;
- do not auto-update production dependencies without verification/canary.

## 11. Secrets

Never place in Git/logs/process command lines:

- Strike API token;
- Strike webhook secret;
- OpenHAB token;
- NIP-46 client secret;
- WireGuard private key;
- TLS private key;
- future payout credentials.

Prefer systemd credentials or root-managed mode-0600 files.

Review journal/application logging for accidental auth header/body leakage.

## 12. Data and backup integrity

For the new SQLite ledger:

- durable WAL/FULL-sync behavior remains enabled as designed;
- back up the database/state regularly enough for the operational importance;
- preserve event/idempotency data together;
- test restore procedure before relying on backups;
- never roll back to an older ledger snapshot without reconciling payments accepted after that snapshot.

## 13. Monitoring

At minimum monitor/log:

- service restarts/crashes;
- Strike API/webhook errors;
- rejected webhook signatures;
- LNURL callback rate-limit/rejection counts;
- unresolved/unknown feed attempts;
- feeder gateway unreachable/denied requests;
- WireGuard handshake health;
- Nostr outbox backlog/failures;
- disk/state backup failures;
- operational Strike balance against ceiling.

Monitoring must not carry production spend authority.

## 14. Production acceptance

This checklist is complete only when the corresponding checks are represented in issue #15 and recorded during staging/cutover.

Any intentionally skipped control should be documented with rationale and compensating control.