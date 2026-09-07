# Phase 1 Execution Plan — Strike-backed Lightning Goats

Status: approved planning context; execution has not started.

Tracker: https://github.com/lightning-goats/lightning-goats/issues/6

## Objective

Replace the production LNbits/Core Lightning path with a small standalone `lightning-goatsd` stack on a new VPS while preserving:

- `herd@lightning-goats.com` Lightning Address payments;
- Strike-backed Lightning receive requests;
- durable exactly-once feed-credit accounting;
- the existing OpenHAB feeder and `FeederOverride` safety model;
- payment-received and feeder-triggered Nostr messages;
- the production video overlay;
- informational/interface/weather messages on the overlay only.

Phase 1 intentionally excludes CyberHerd membership, headbutts, NIP-05 verification, rewards/distributions, and spend-capable Lightning/Strike authority.

## Approved deployment strategy

Build the replacement stack on a **new VPS in parallel** with the current production VPS. The old VPS stays authoritative until the replacement passes the full verification matrix.

The new VPS receives its own WireGuard identity during staging. Existing WireGuard clients continue using the old VPS while testing proceeds. At production cutover, client peer configuration is changed to the new VPS public key/endpoint (unless the operator explicitly chooses a different topology).

Do not reuse the old VPS WireGuard private key while both hosts are online.

Production DNS remains pointed at the old VPS until the new stack is accepted.

## Accounts and privilege model

Use two Unix identities:

### Deployment/Codex account

Suggested name: `lg-deploy` or `codex`.

During staging it may:

- log in interactively;
- own the source checkout/worktree;
- run Codex, Rust tooling, tests, and diagnostics;
- receive **temporary sudo** for host provisioning and configuration.

Before production credentials are installed and before final cutover:

- revoke broad sudo;
- audit its filesystem and SSH access;
- ensure it cannot read production systemd credentials;
- retain it only as a non-privileged maintenance/development account if desired.

### Production runtime account

Suggested name: `lightning-goats`.

It must:

- have no sudo/admin privileges;
- preferably have no interactive login shell;
- not own system configuration or installed production binaries;
- receive only the runtime credentials required by `lightning-goatsd`;
- run via a system-level systemd service using `User=lightning-goats`.

Do not run Codex as the production runtime identity.

## Recommended work order

The GitHub issues are the executable work queue.

1. #7 — backend-neutral payment/ledger domain.
2. #8 — receive-only Strike API integration and settlement reconciliation.
3. #9 — native Lightning Address/LNURL-pay endpoints.
4. #10 — Phase 1 message templates.
5. #11 — templated durable events to Nostr + overlay.
6. #12 — CyberHerd-ready service/event boundaries.
7. #13 — remove CLN/LNbits/clnaddress runtime assumptions.
8. #14 — VPS/nginx/WireGuard/credential hardening.
9. #15 — full end-to-end verification matrix.
10. #16 — production cutover and rollback runbook.

Issues #10, #12, and much of #14 may proceed in parallel with #7–#9, but #16 is gated on #15.

## Phase 1 architecture invariants

- Strike is the only Lightning backend.
- `lightning-goatsd` has receive/read Strike authority only; it cannot spend or withdraw.
- Strike webhooks are notifications, not authoritative settlement records. Verify the webhook, then fetch authoritative Strike state before crediting.
- Duplicate/replayed notifications never create duplicate feed credit.
- Payment settlement + feed credit + durable `payment_received` event commit atomically.
- Feeder actuation is serialized.
- An ambiguous physical feeder outcome becomes `unknown` and is never automatically retried.
- `FeederOverride` remains fail-safe.
- Presentation failures never roll back or duplicate financial/feed state.
- Payment and feeder messages go to Nostr + overlay.
- Informational/interface/weather messages go to overlay only.
- Nostr signing remains isolated through `nak`/NIP-46.
- No production LNbits, CLNRest, Core Lightning, `clnaddress`, or CLN `pay_index` dependency remains after cutover.

## Staging milestones

### M0 — New host bootstrapped

- new VPS provisioned;
- deploy/Codex account created;
- temporary sudo enabled;
- Codex and Rust toolchain installed;
- repo cloned;
- new VPS added to WireGuard with its own keypair;
- no production DNS changes;
- no production secrets installed.

### M1 — Backend-neutral service builds

- #7 merged;
- existing feeder accounting tests remain green;
- no production CLN cursor dependency remains in the new code path.

### M2 — Strike + Lightning Address path works

- #8 and #9 merged;
- test Lightning Address resolves against staging hostname/host override;
- Strike receive request returns valid BOLT11;
- signed webhook + authoritative reconciliation credits exactly once.

### M3 — Production-visible behavior reproduced

- #10 and #11 merged;
- fun goat-fact payment messages work;
- feeder-trigger messages work;
- Nostr durable outbox works;
- overlay reconnect/replay works;
- informational messages are overlay-only.

### M4 — Legacy runtime removed and host hardened

- #12–#14 complete;
- no CLN/LNbits secrets/processes required;
- nginx/systemd/WireGuard rules installed;
- production runtime account created;
- deploy account broad sudo revoked before final secrets are installed.

### M5 — Canary accepted

- #15 complete;
- tiny real Strike payment succeeds on staging path;
- exactly-once ledger event verified;
- Nostr + overlay verified;
- harmless OpenHAB canary verified;
- controlled physical feeder test verified.

### M6 — Production cutover

Execute #16 only after operator approval.

## Required verification commands

For Rust changes:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
```

Also run the repository security/audit workflow documented in `docs/implementation-status.md` plus task-specific integration/failure-injection tests.

## Stop conditions

Codex must stop and require operator action before:

- changing production DNS;
- changing existing clients to the new WireGuard hub;
- disabling the old production VPS;
- installing/rotating final production secrets if broad temporary sudo is still present;
- releasing `FeederOverride` for a real physical feed;
- granting any Strike spend authority.

These are production cutover decisions, not autonomous implementation steps.
