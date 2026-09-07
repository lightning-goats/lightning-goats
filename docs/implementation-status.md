# Phase 1 Implementation Status

Last implementation update: 2026-09-07.

Tracker: https://github.com/lightning-goats/lightning-goats/issues/6

Deployment handoff: [`docs/deployment/codex-handoff.md`](deployment/codex-handoff.md)

## Target now implemented in the Phase 1 PR stack

The final Phase 1 runtime is:

```text
Internet
  -> nginx/TLS + static lightning-goats.com on new VPS
  -> lightning-goatsd on 127.0.0.1
       -> Strike receive/read only
       -> native LNURL-pay / six Lightning Addresses
       -> durable SQLite feed accounting
       -> NIP-46/NAK Nostr publishing
       -> overlay websocket
       -> narrow GatewayClient
  -> WireGuard 10.8.0.0/24
       -> 10.8.0.6:8789 lightning-goats-gateway
            -> dedicated OpenHAB USER token
            -> existing correlated feeder owner
            -> local feeder safety/rate history
            -> localhost weather read
```

There is no intended production runtime dependency on LNbits, Core Lightning, CLNRest, `clnaddress`, a CLN rune, or a VPS-side OpenHAB token.

## Implementation / PR stack

- **#7 backend-neutral ledger** — implemented and merged in PR #22.
- **#8 Strike receive/reconciliation** — implemented in PR #24; CI/Security green and ready for review.
- **#9 + #18 native LNURL-pay + six-address registry** — implemented in PR #25; CI/Security green and ready for review.
- **#10 + #11 Phase 1 templates + Nostr/overlay fan-out** — implemented in PR #27; deterministic data-driven rendering, Nostr/overlay audience separation, and durable outbox semantics preserved.
- **#17 + #21 trusted OpenHAB/weather gateway** — implemented in PR #28; CI cleanup/final verification is the current gate.
- **#13 remove CLN/LNbits runtime compatibility** — implemented in PR #29; makes Strike/LNURL mandatory, removes CLN source/config/secrets/cursor/watcher, and adds an upgrade migration dropping obsolete CLN tables. CI cleanup/final verification is the current gate.
- **Deployment handoff / final documentation** — `phase1/deployment-handoff`; includes Codex handoff, host preflights and final documentation consistency work.
- **#26 public-site migration** — requirements are implemented as a deployment plan, but the actual current production `index.html` and assets intentionally cannot be finalized from GitHub alone. Codex must copy the authoritative source from the old VPS during staging, commit/review it under `web/`, and apply the documented Phase 1 edits.

Do not merge or deploy a stacked child PR while its required parent changes are absent from the target branch.

## Durable financial/feeder invariants

Implemented behavior includes:

- provider-neutral `(source, source_id)` settlement identity;
- payment-hash collision protection;
- exact duplicate delivery is idempotent;
- non-sat-aligned settlement is rejected rather than truncated;
- settlement + `HERD_RECEIPT` + `payment_received` event is atomic;
- shared `herd` credit pool while preserving `address_user`;
- serialized feed intents and multi-threshold drain;
- `2340 sats -> 2 confirmed feeds -> 340 sats remainder` behavior;
- interrupted/ambiguous physical action becomes `unknown` and blocks automatic retry;
- operator reconciliation never directly actuates the feeder.

## Strike / Lightning Address boundary

Required configured addresses:

```text
herd@lightning-goats.com
dexter@lightning-goats.com
rowan@lightning-goats.com
cosmo@lightning-goats.com
newton@lightning-goats.com
nova@lightning-goats.com
```

All six use `credit_pool=herd` and preserve the actual user. Unknown users and invalid/non-whole-sat callback amounts fail before provider contact.

Runtime Strike credential scope is receive/read only. Webhook-management authority is separate and temporary. A webhook is only a signed notification; authoritative receive state is fetched from Strike before durable credit.

## Trusted physical/weather boundary

The approved production boundary is implemented in the gateway code:

```text
lightning-goatsd (VPS; no OpenHAB token)
    -> source-restricted WireGuard/UFW path
    -> 10.8.0.6:8789 lightning-goats-gateway
    -> dedicated OpenHAB USER token on trusted host only
    -> existing correlated feeder owner + sanitized local weather
```

The gateway:

- persists the feeder UUID before issuing any command;
- never automatically resends a pending/ambiguous UUID;
- accepts an authoritative ack only when it matches the request UUID and reports explicit successful/completed outcome;
- supports a tightly validated `{request_id}` request payload template so deployment can bind to the exact live `GoatFeeder_ManualRequest` contract;
- enforces a local minimum feed interval and rolling feeds-per-hour cap in addition to the existing OpenHAB owner policy;
- fails closed on `FeederOverride`, `LightningGoatsRemoteEnabled`, malformed result state, OpenHAB failure, or unavailable trusted state;
- reads weather only from `http://127.0.0.1:5000/get_received_data` and never exposes the legacy mutating `/weather` endpoint.

The live OpenHAB feeder owner must be inspected during deployment to resolve the exact correlated result Item and request JSON format. This is environment binding, not unfinished architecture.

## Messaging / presentation

Implemented Phase 1 presentation categories:

```text
payment_received -> sats_received  -> Nostr + overlay
feeder_confirmed -> feeder_trigger -> Nostr + overlay
interface_info   -> overlay only
weather_status   -> overlay only
```

Template and goat selection are deterministic from durable event identity so restart/retry gives the same presentation. Individual goat-address payments use the paid goat. Nostr gets goat Nostr references; the overlay gets display names and image metadata.

The informational scheduler preserves the current implementation behavior:

- default interval 60 seconds;
- interface-info unconditional probability 0.40;
- weather unconditional probability 0.40;
- interface-info considered first;
- at most one informational message per cycle.

## Public website

Issue #26 is a deployment-time migration because the authoritative page exists on the old VPS rather than in this repository.

During staging Codex must copy the current source/assets into `web/`, then:

- remove NIP-05 Verify UI/modal/iframe and all legacy NIP-05/LNbits calls;
- hide/disable the Phase-2 CyberHerd leaderboard and all legacy CyberHerd requests;
- keep live stream and Nostr chat where independent of retired services;
- keep browser-extension signing; never request raw nsec;
- replace email contact with an operator-confirmed Nostr DM/pubkey path;
- use native Lightning Address/LNURL payment paths;
- retain zap controls only when the replacement invoice flow is verified independent of LNbits/CLN.

## Deployment artifacts present

The Phase 1 branches contain:

- production/canary `lightning-goatsd` configs and systemd units;
- production/canary trusted gateway configs and systemd units;
- nginx production/canary route examples and public ingress rate limits;
- source-specific trusted-host UFW template;
- `deploy/checks/gateway-preflight.sh`;
- `deploy/checks/vps-preflight.sh`;
- parallel staging, WireGuard, hardening, website, verification, cutover and rollback docs;
- `docs/deployment/codex-handoff.md` as the deployment entry point.

Neither preflight performs a physical feeder action.

## Work that remains by design for Codex / operator deployment

The remaining tasks require live host/account state rather than more architectural design:

1. provision the new VPS and inventory an unused staging `10.8.0.x` address;
2. install the reviewed binaries/units/nginx/WireGuard configuration;
3. create the dedicated OpenHAB USER/token on the trusted host;
4. inspect the live feeder owner and bind its exact result Item/request payload in gateway config;
5. create/verify the remote-enable and harmless canary Items/rule;
6. apply and verify source-specific UFW containment;
7. configure Strike sandbox, then production receive/read credentials and webhook;
8. configure the existing NIP-46 client values;
9. copy and modify the authoritative public website source;
10. execute the full staging matrix and produce the deployment evidence report;
11. revoke/narrow temporary Codex/deploy sudo before final production secrets/cutover;
12. obtain explicit operator approval for production WireGuard/DNS switch and the one controlled physical feeder canary;
13. observe production before retiring the old VPS.

## Operator gates

Do not execute without explicit operator direction:

- stop old production WireGuard;
- move/reassign `10.8.0.1` or repoint production clients;
- change production DNS;
- enable a physical feeder request from the new stack;
- change safety limits or the operator-defined Strike balance ceiling;
- grant spend-capable Strike authority;
- destroy old VPS state/backups/CLN recovery material.

## Required software gate

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
cargo tree -i rsa --locked
cargo audit --ignore RUSTSEC-2023-0071
```

`RUSTSEC-2023-0071` may remain ignored only while `rsa` is unreachable from the active application dependency graph. If `rsa` becomes reachable, fail the security gate rather than relying on the exception.

## Deployment gate

A deployment is not ready for cutover until both preflights and every mandatory item in `docs/testing/phase1-verification-matrix.md` pass and the evidence package required by `docs/deployment/codex-handoff.md` is complete.
