# Phase 1 Implementation Status

Last planning update: 2026-09-07.

Tracker: https://github.com/lightning-goats/lightning-goats/issues/6

## Current direction

The earlier CLNRest + `clnaddress` production cutover design has been superseded.

The approved Phase 1 target is now:

- native Lightning Address / LNURL-pay endpoints in `lightning-goatsd`;
- configured Lightning Addresses for `herd`, `dexter`, `rowan`, `cosmo`, `newton`, and `nova`;
- all configured Phase 1 addresses credit the same `herd` feeder pool while preserving the actual paid `address_user`;
- Strike API as the only Lightning/payment backend;
- receive/read-only Strike authority only;
- no LNbits production dependency;
- no Core Lightning / CLNRest / `clnaddress` production dependency;
- no OpenHAB API token on the public VPS;
- a narrow in-house feeder gateway with a dedicated OpenHAB USER/token, request UUID/ack protocol, local duplicate suppression, `FeederOverride`, remote-enable, minimum interval and safety/feed-cap enforcement;
- dedicated WireGuard/UFW containment so the VPS can reach only the required feeder gateway rather than general OpenHAB/LAN services;
- existing durable feed accounting, Nostr outbox, and video overlay preserved;
- payment and feeder messages rendered from the existing fun goat-fact template style;
- informational/interface/weather messages sent to the overlay only;
- public LNURL/webhook abuse controls;
- domain/DNS, SSH, deployment provenance, and operational Strike-balance hardening before cutover;
- clean Phase 2 seam for future CyberHerd functionality.

See `docs/README.md` for the canonical documentation set.

## Existing implementation that should be preserved

The repository already contains substantial reusable Phase 1 functionality:

- Rust service with `#![forbid(unsafe_code)]`;
- durable SQLite database using WAL, `synchronous=FULL`, foreign keys, and migrations;
- feed-credit ledger;
- serialized multi-threshold feed accounting;
- persistent feed intents;
- ambiguous/interrupted feed handling that blocks automatic retry;
- local operator feed reconciliation;
- current OpenHAB adapter behavior and `FeederOverride` semantics as useful reference while replacing direct VPS->OpenHAB access with #17;
- optional status/temperature presentation behavior;
- durable event log;
- read-only overlay WebSocket with snapshot/replay/sequence behavior;
- read-only health/status endpoints;
- NIP-46/`nak` signing adapter;
- transactional signed-event Nostr outbox;
- exact signed-event retry semantics;
- shadow/canary/active safety concepts;
- hardened system-level systemd unit examples;
- release workflow and locked Rust verification gates.

These are assets to refactor around, not reasons to preserve the CLN-specific payment ingress or direct OpenHAB credential placement.

## CLN/LNbits-specific implementation to retire

Once the Strike path is implemented and verified, remove or migrate away from:

- `src/cln/`;
- `invoice_watcher` / CLNRest `waitanyinvoice`;
- CLN `pay_index` cursor/startup requirement;
- `clnaddress:v1:*` label classification;
- CLN rune/TLS credential requirements;
- `lightning-goatsctl init-cursor`;
- CLN-specific database columns/tables where no longer useful;
- production deployment/runbook assumptions requiring LNbits, CLNRest, Core Lightning, or `clnaddress`.

Issue #13 tracks this cleanup.

## Direct OpenHAB integration to replace

The current Rust client directly reads OpenHAB Items and executes a rule. That implementation is no longer the approved production trust boundary.

Issue #17 replaces production direct access with:

```text
lightning-goatsd (VPS, no OpenHAB token)
    -> narrow WireGuard/UFW path
    -> in-house feeder gateway
    -> dedicated OpenHAB USER/token
    -> dedicated request/ack Items and local safety rule
```

The existing durable `unknown`/no-blind-retry feeder behavior must be preserved and strengthened by local duplicate UUID suppression.

## Phase 1 work status

At this planning checkpoint, the new Strike-backed implementation work has not begun.

Open work queue:

- [ ] #7 Backend-neutral payment/ledger domain
- [ ] #8 Receive-only Strike integration and settlement reconciliation
- [ ] #9 Native Lightning Address/LNURL-pay endpoints
- [ ] #18 Configured herd + individual goat Lightning Address registry
- [ ] #10 Port Phase 1 message templates
- [ ] #11 Wire templates to Nostr + video overlay
- [ ] #12 CyberHerd-ready service/event boundaries
- [ ] #17 Narrow OpenHAB feeder gateway + dedicated WireGuard/UFW boundary
- [ ] #13 Remove CLN/LNbits/clnaddress runtime assumptions
- [ ] #14 Harden new VPS/nginx/WireGuard/credentials
- [ ] #19 Harden domain/DNS, SSH, deployment provenance, and operational Strike balance
- [ ] #20 Public LNURL/webhook abuse controls
- [ ] #15 End-to-end verification matrix
- [ ] #16 Production cutover and rollback runbook execution

## Deployment state

Approved deployment method:

1. provision a new VPS;
2. add it to the existing WireGuard network using a **new WireGuard keypair/peer identity**;
3. prefer a dedicated/narrow application WireGuard path for VPS -> in-house feeder gateway;
4. create a separate Codex/deployment Unix account with temporary sudo;
5. implement and test the replacement stack on the new VPS in parallel;
6. build the in-house feeder gateway and dedicated OpenHAB integration identity/token without copying that token to the VPS;
7. keep the old VPS and production DNS authoritative during staging;
8. run production `lightning-goatsd` as a system-level systemd service under a separate non-admin runtime account;
9. run the feeder gateway as a separate non-admin in-house system service;
10. capture privileged host configuration in reviewed/reproducible repo assets where practical;
11. revoke/narrow broad deploy/Codex sudo before final production secrets/cutover;
12. harden SSH/domain/DNS/public ingress and record deployment provenance;
13. pass the complete verification matrix in `docs/testing/phase1-verification-matrix.md`;
14. with explicit operator approval, repoint required WireGuard clients and production DNS to the new VPS;
15. verify all configured Lightning Addresses, one tiny real payment, and a controlled gateway-mediated feeder cycle;
16. keep the old VPS intact as rollback/archive during an observation period;
17. retire/destroy the old VPS only after successful observation and backup verification.

## Phase 1 Lightning Address scope

Required addresses:

```text
herd@lightning-goats.com
dexter@lightning-goats.com
rowan@lightning-goats.com
cosmo@lightning-goats.com
newton@lightning-goats.com
nova@lightning-goats.com
```

All six use `credit_pool=herd` while preserving `address_user`.

Nginx can route generic LNURL user paths, but the application registry is authoritative. Unknown users fail before provider contact.

See `docs/architecture/lightning-address-registry.md`.

## Phase 1 message scope

Port only:

- `sats_received` — payment-received fun goat-fact templates;
- `feeder_trigger` — feeder-trigger fun goat-fact templates;
- informational/interface/weather templates needed by the overlay.

Audience:

```text
payment_received  -> Nostr + overlay
feeder_confirmed  -> Nostr + overlay
informational     -> overlay only
```

Do not port CyberHerd membership/headbutt/reward/distribution templates in Phase 1.

## Security hardening state

Required before production:

- #17 physical feeder/OpenHAB boundary;
- #14 VPS/network/credential/systemd hardening;
- #19 domain/DNS/SSH/deployment provenance/operational balance controls;
- #20 public LNURL/webhook abuse controls;
- #15 functional/security verification matrix.

Canonical controls are documented in:

- `docs/security/phase1-threat-model.md`;
- `docs/security/openhab-feeder-gateway.md`;
- `docs/security/phase1-hardening-checklist.md`;
- `docs/testing/phase1-verification-matrix.md`.

## Phase 2 boundary

Phase 1 must remain extensible so CyberHerd can later be implemented either as:

- a separate service such as `cyberherdd`; or
- an internal module in the Lightning Goats codebase.

Do not grant spend-capable Strike authority to `lightning-goatsd` in anticipation of Phase 2. If future rewards require outbound payments, prefer a separately privileged payout component.

Future CyberHerd logic must not bypass the feeder gateway or gain direct OpenHAB credentials.

See `docs/architecture/cyberherd-phase2-boundary.md`.

## Required locked Rust gate

Run for implementation changes:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
```

Security audit gate:

```sh
cargo tree -i rsa --locked
cargo audit --ignore RUSTSEC-2023-0071
```

`RUSTSEC-2023-0071` should only remain ignored while `rsa` is unreachable from the active application dependency graph. Remove the exception when the dependency resolution permits it. If `rsa` becomes reachable, fail the security gate rather than relying on the ignore.

## Definition of done

Phase 1 is complete when:

- all six configured Lightning Addresses resolve natively through LNURL-pay;
- unknown users fail closed before provider contact;
- Strike is the only Lightning backend;
- a completed receive is independently reconciled and credited exactly once with correct recipient metadata;
- `lightning-goatsd` has no OpenHAB token/direct generic OpenHAB access;
- feeder threshold/remainder/ambiguity semantics are preserved through the narrow gateway;
- duplicate feeder request UUID cannot actuate twice;
- local OpenHAB physical safety gates are verified;
- payment and feeder events produce the intended Nostr + overlay messages;
- informational messages are overlay-only;
- public ingress/network/SSH/domain/deployment hardening checks pass;
- an operator-approved operational Strike balance ceiling/sweep policy is active;
- the new VPS passes the full staging matrix;
- production WireGuard/DNS cutover has been performed using the runbook;
- LNbits/CLN/clnaddress are no longer in the live production payment path;
- old VPS state and CLN recovery material are archived as required.
