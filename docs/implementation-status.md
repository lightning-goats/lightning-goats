# Phase 1 Implementation Status

Last implementation update: 2026-09-07.

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
- public `lightning-goats.com` static site served by the new VPS, with NIP-05/legacy LNbits UI removed and Phase-2-only leaderboard disabled until CyberHerd returns;
- public LNURL/webhook abuse controls;
- domain/DNS, SSH, deployment provenance, and operational Strike-balance hardening before cutover;
- clean Phase 2 seam for future CyberHerd functionality.

See `docs/README.md` for the canonical documentation set.

## Implemented / in review

- #7 backend-neutral payment/ledger domain: implemented and merged via PR #22.
- #8 receive-only Strike integration and settlement reconciliation: implemented; mainline PR #24 is green and ready for review.
- #9 + #18 native LNURL-pay and the six-address registry: implemented in PR #25; CI and Security are green and the PR is ready for review.
- #10 + #11 deterministic data-driven payment/feeder/interface templates and shared Nostr/overlay rendering: implemented in draft PR #27; CI/Security are the current gate.
- #26 public-site migration/simplification: requirements documented; authoritative current `index.html` and assets must be copied from the old VPS into the repo during new-VPS staging before modification/deployment.

## Existing implementation that should be preserved

The repository already contains substantial reusable Phase 1 functionality:

- Rust service with `#![forbid(unsafe_code)]`;
- durable SQLite database using WAL, `synchronous=FULL`, foreign keys, and migrations;
- feed-credit ledger;
- serialized multi-threshold feed accounting;
- persistent feed intents;
- ambiguous/interrupted feed handling that blocks automatic retry;
- local operator feed reconciliation;
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

Once the Strike/LNURL path is merged and verified, remove or migrate away from:

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

## Remaining Phase 1 work queue

- [ ] #12 CyberHerd-ready service/event boundaries
- [ ] #17 Narrow OpenHAB feeder gateway + dedicated WireGuard/UFW boundary
- [ ] #21 Weather overlay compatibility through the in-house gateway
- [ ] #26 Migrate/simplify public `lightning-goats.com` site
- [ ] #13 Remove CLN/LNbits/clnaddress runtime assumptions
- [ ] #14 Harden new VPS/nginx/WireGuard/credentials
- [ ] #19 Harden domain/DNS, SSH, deployment provenance, and operational Strike balance
- [ ] #20 Public LNURL/webhook abuse controls
- [ ] #15 End-to-end verification matrix
- [ ] #16 Production cutover and rollback runbook execution

## Deployment state

Approved deployment method:

1. provision a new VPS;
2. add it to the existing `10.8.0.0/24` WireGuard network using a **new WireGuard keypair/peer identity** and temporary unused `10.8.0.x` address;
3. create a separate Codex/deployment Unix account with temporary sudo;
4. implement/test the replacement stack on the new VPS in parallel while old `10.8.0.1` and production DNS remain authoritative;
5. copy the authoritative existing `lightning-goats.com` static source/assets from the old VPS into this repository, then apply #26 changes and stage them on the new VPS;
6. build the in-house feeder/weather gateway and dedicated OpenHAB integration identity/token without copying that token to the VPS;
7. run production `lightning-goatsd` as a system-level systemd service under a separate non-admin runtime account;
8. capture privileged host configuration in reviewed/reproducible repo assets where practical;
9. revoke/narrow broad deploy/Codex sudo before final production secrets/cutover;
10. harden SSH/domain/DNS/public ingress and record deployment provenance;
11. pass the complete verification matrix in `docs/testing/phase1-verification-matrix.md`;
12. with explicit operator approval, stop old hub WireGuard, move the new hub to `10.8.0.1` if retaining that address, update clients to the new hub key/Internet endpoint, and switch production DNS;
13. verify the public site, all six Lightning Addresses, one tiny real payment, Nostr + overlay presentation, weather, and a controlled gateway-mediated feeder cycle;
14. keep the old VPS intact as rollback/archive during an observation period;
15. retire/destroy the old VPS only after successful observation and backup verification.

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

Only these presentation categories are implemented:

- `sats_received` — payment-received goat-fact templates;
- `feeder_trigger` — feeder-trigger goat-fact templates;
- `interface_info` — overlay-only informational templates;
- `weather_status` — overlay-only preformatted weather messages (construction/polling remains #21).

Audience:

```text
payment_received  -> Nostr + overlay
feeder_confirmed  -> Nostr + overlay
interface_info    -> overlay only
weather_status    -> overlay only
```

Template/goat selection is deterministic from the durable event identity so restart/retry renders the same presentation. Individual goat-address payments use the paid goat. Nostr uses goat Nostr profile mentions; the overlay uses human-readable names/image metadata.

See `docs/architecture/phase1-messaging.md`.

## Public website scope

Issue #26 owns migration of the current production site to the new VPS. The original production `index.html`/assets should be copied from the old VPS rather than reconstructed from the public rendering.

Phase 1 removes NIP-05 verification and legacy LNbits/CyberHerd requests. Contact becomes a Nostr DM/pubkey path using an operator-confirmed public key. The live CyberHerd leaderboard is hidden/disabled until Phase 2. Live stream and Nostr chat should remain where independent of retired services.

See `docs/deployment/public-site-migration.md`.

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
- informational/weather messages are overlay-only;
- the migrated static website is live from the new VPS without legacy NIP-05/LNbits/CyberHerd requests;
- public ingress/network/SSH/domain/deployment hardening checks pass;
- an operator-approved operational Strike balance ceiling/sweep policy is active;
- the new VPS passes the full staging matrix;
- production WireGuard/DNS cutover has been performed using the runbook;
- LNbits/CLN/clnaddress are no longer in the live production payment path;
- old VPS state and CLN recovery material are archived as required.
