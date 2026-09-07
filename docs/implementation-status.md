# Phase 1 Implementation Status

Last planning update: 2026-09-07.

Tracker: https://github.com/lightning-goats/lightning-goats/issues/6

## Current direction

The earlier CLNRest + `clnaddress` production cutover design has been superseded.

The approved Phase 1 target is now:

- native Lightning Address / LNURL-pay endpoints in `lightning-goatsd`;
- Strike API as the only Lightning/payment backend;
- receive/read-only Strike authority only;
- no LNbits production dependency;
- no Core Lightning / CLNRest / `clnaddress` production dependency;
- existing durable feed accounting and OpenHAB feeder safety preserved;
- existing durable Nostr outbox and video overlay preserved;
- payment and feeder messages rendered from the existing fun goat-fact template style;
- informational/interface/weather messages sent to the overlay only;
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
- OpenHAB feeder adapter and `FeederOverride` safety behavior;
- optional OpenHAB status/temperature reads;
- durable event log;
- read-only overlay WebSocket with snapshot/replay/sequence behavior;
- read-only health/status endpoints;
- NIP-46/`nak` signing adapter;
- transactional signed-event Nostr outbox;
- exact signed-event retry semantics;
- shadow/canary/active safety concepts;
- hardened system-level systemd unit examples;
- release workflow and locked Rust verification gates.

These are assets to refactor around, not reasons to preserve the CLN-specific payment ingress.

## CLN-specific implementation to retire

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

## Phase 1 work status

At this planning checkpoint, the new Strike-backed implementation work has not begun.

Open work queue:

- [ ] #7 Backend-neutral payment/ledger domain
- [ ] #8 Receive-only Strike integration and settlement reconciliation
- [ ] #9 Native Lightning Address/LNURL-pay endpoints
- [ ] #10 Port Phase 1 message templates
- [ ] #11 Wire templates to Nostr + video overlay
- [ ] #12 CyberHerd-ready service/event boundaries
- [ ] #13 Remove CLN/LNbits/clnaddress runtime assumptions
- [ ] #14 Harden new VPS/nginx/WireGuard/credentials
- [ ] #15 End-to-end verification matrix
- [ ] #16 Production cutover and rollback runbook execution

## Deployment state

Approved deployment method:

1. provision a new VPS;
2. add it to the existing WireGuard network using a **new WireGuard keypair/peer identity**;
3. create a separate Codex/deployment Unix account with temporary sudo;
4. implement and test the replacement stack on the new VPS in parallel;
5. keep the old VPS and production DNS authoritative during staging;
6. run production `lightning-goatsd` as a system-level systemd service under a separate non-admin runtime account;
7. revoke broad deploy/Codex sudo before final production secrets/cutover;
8. pass the complete canary/verification matrix;
9. with explicit operator approval, repoint required WireGuard clients and production DNS to the new VPS;
10. verify one tiny real payment and controlled feeder cycle;
11. keep the old VPS intact as rollback/archive during an observation period;
12. retire/destroy the old VPS only after successful observation and backup verification.

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

## Phase 2 boundary

Phase 1 must remain extensible so CyberHerd can later be implemented either as:

- a separate service such as `cyberherdd`; or
- an internal module in the Lightning Goats codebase.

Do not grant spend-capable Strike authority to `lightning-goatsd` in anticipation of Phase 2. If future rewards require outbound payments, prefer a separately privileged payout component.

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

- `herd@lightning-goats.com` resolves natively through LNURL-pay;
- Strike is the only Lightning backend;
- a completed receive is independently reconciled and credited exactly once;
- feeder threshold/remainder/ambiguity semantics are preserved;
- payment and feeder events produce the intended Nostr + overlay messages;
- informational messages are overlay-only;
- the new VPS passes the full staging matrix;
- production WireGuard/DNS cutover has been performed using the runbook;
- LNbits/CLN/clnaddress are no longer in the live production payment path;
- old VPS state and CLN recovery material are archived as required.
