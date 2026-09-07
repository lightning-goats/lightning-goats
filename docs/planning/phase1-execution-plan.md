# Phase 1 Execution Plan — Strike-backed Lightning Goats

Status: approved planning context; execution has not started.

Tracker: https://github.com/lightning-goats/lightning-goats/issues/6

## Objective

Replace the production LNbits/Core Lightning path with a small standalone `lightning-goatsd` stack on a new VPS while preserving:

- `herd@lightning-goats.com` Lightning Address payments;
- individual goat Lightning Addresses: `dexter`, `rowan`, `cosmo`, `newton`, `nova`;
- Strike-backed Lightning receive requests;
- durable exactly-once feed-credit accounting;
- the existing feeder and `FeederOverride` safety behavior, strengthened by an in-house feeder gateway and local OpenHAB command/ack safeguards;
- payment-received and feeder-triggered Nostr messages;
- the production video overlay;
- informational/interface/weather messages on the overlay only.

Phase 1 intentionally excludes CyberHerd membership, headbutts, NIP-05 verification, rewards/distributions, and spend-capable Lightning/Strike authority.

## Approved deployment strategy

Build the replacement stack on a **new VPS in parallel** with the current production VPS. The old VPS stays authoritative until the replacement passes the full verification matrix.

The new VPS receives its own WireGuard identity during staging. Existing WireGuard clients continue using the old VPS while testing proceeds. At production cutover, client peer configuration is changed to the new VPS public key/endpoint (unless the operator explicitly chooses a different topology).

For the Lightning Goats application-to-home feeder path, prefer a dedicated WireGuard interface/key/subnet or equivalently strong per-peer firewall isolation. The VPS must not have generic OpenHAB/LAN access.

Do not reuse the old VPS WireGuard private key while both hosts are online.

Production DNS remains pointed at the old VPS until the new stack is accepted.

## Accounts and privilege model

Use separate deployment and runtime identities.

### Deployment/Codex account

Suggested name: `lg-deploy` or `codex`.

During staging it may:

- log in interactively;
- own the source checkout/worktree;
- run Codex, Rust tooling, tests, and diagnostics;
- receive **temporary sudo** for host provisioning and configuration.

Every privileged host change should be captured in reviewed/reproducible repo scripts/configuration where practical.

Before production credentials are installed and before final cutover:

- revoke/narrow broad sudo;
- audit sudoers, filesystem ownership and SSH access;
- ensure it cannot read production systemd credentials;
- ensure it cannot modify the installed production binary/configuration;
- optionally rebuild/reimage the final VPS from reviewed artifacts for maximum assurance.

### Production runtime account

Suggested name: `lightning-goats`.

It must:

- have no sudo/admin privileges;
- preferably have no interactive login shell;
- not own system configuration or installed production binaries;
- receive only the runtime credentials required by `lightning-goatsd`;
- run via a system-level systemd service using `User=lightning-goats`.

It must **not** receive an OpenHAB token. OpenHAB credentials live only on the trusted in-house feeder gateway.

Do not run Codex as the production runtime identity.

## Recommended work order

The GitHub issues are the executable work queue.

1. #7 — backend-neutral payment/ledger domain.
2. #8 — receive-only Strike API integration and settlement reconciliation.
3. #9 — native Lightning Address/LNURL-pay endpoints.
4. #18 — configured registry for herd + five individual goat Lightning Addresses.
5. #10 — Phase 1 message templates.
6. #11 — templated durable events to Nostr + overlay.
7. #12 — CyberHerd-ready service/event boundaries.
8. #17 — in-house OpenHAB feeder gateway + dedicated WireGuard/UFW boundary.
9. #13 — remove CLN/LNbits/clnaddress runtime assumptions.
10. #14 — VPS/nginx/WireGuard/credential hardening.
11. #19 — domain/DNS, SSH, deployment provenance and operational Strike balance hardening.
12. #20 — LNURL/webhook abuse controls.
13. #15 — full end-to-end verification matrix.
14. #16 — production cutover and rollback runbook.

Issues #10, #12, #17, #19 and much of #14 may proceed in parallel with the payment work. #18 follows the generic LNURL work in #9. #16 is gated on #15 and the completion/operator acceptance of the security-boundary issues.

## Phase 1 architecture invariants

- Strike is the only Lightning backend.
- `lightning-goatsd` has receive/read Strike authority only; it cannot spend or withdraw.
- Strike webhooks are notifications, not authoritative settlement records. Verify the webhook, then fetch authoritative Strike state before crediting.
- Duplicate/replayed notifications never create duplicate feed credit.
- Payment settlement + feed credit + durable `payment_received` event commit atomically.
- Only configured Lightning Address users may create provider invoices; generic nginx routing is not wildcard payment authorization.
- `herd`, `dexter`, `rowan`, `cosmo`, `newton`, and `nova` all credit the same feeder pool while preserving `address_user` metadata.
- Feeder actuation is serialized and uses a durable UUID command/ack path through the in-house gateway.
- `lightning-goatsd` holds no OpenHAB token and has no generic OpenHAB REST access.
- Local OpenHAB safety rules enforce remote-enable, `FeederOverride`, duplicate suppression, minimum physical-feed interval and an absolute safety/feed cap.
- An ambiguous physical feeder outcome becomes `unknown` and is never automatically retried with a new actuation.
- Presentation failures never roll back or duplicate financial/feed state.
- Payment and feeder messages go to Nostr + overlay.
- Informational/interface/weather messages go to overlay only.
- Nostr signing remains isolated through `nak`/NIP-46.
- Public LNURL callback and webhook ingress is method/body/rate constrained.
- No production LNbits, CLNRest, Core Lightning, `clnaddress`, or CLN `pay_index` dependency remains after cutover.

## Staging milestones

### M0 — New host bootstrapped

- new VPS provisioned;
- deploy/Codex account created;
- temporary sudo enabled;
- Codex and Rust toolchain installed;
- repo cloned;
- new VPS added to WireGuard with its own keypair;
- dedicated/narrow feeder application tunnel design prepared;
- no production DNS changes;
- no final production secrets installed.

### M1 — Backend-neutral service builds

- #7 merged;
- existing feeder accounting tests remain green;
- no production CLN cursor dependency remains in the new code path.

### M2 — Strike + configured Lightning Addresses work

- #8, #9 and #18 merged;
- all six configured addresses resolve against staging hostname/host override;
- unknown users fail before provider contact;
- Strike receive request returns valid BOLT11;
- signed webhook + authoritative reconciliation credits exactly once;
- paid `address_user` is durable while all six use the same feeder pool.

### M3 — Production-visible behavior reproduced

- #10 and #11 merged;
- fun goat-fact payment messages work;
- feeder-trigger messages work;
- Nostr durable outbox works;
- overlay reconnect/replay works;
- informational messages are overlay-only.

### M4 — Feeder security boundary established

- #17 complete;
- dedicated OpenHAB integration USER/token exists only on trusted gateway host;
- VPS can reach feeder gateway but cannot reach generic OpenHAB REST/admin/LAN services;
- request UUID/ack path works;
- duplicate UUID does not actuate twice;
- local physical safety gates are authoritative.

### M5 — Legacy runtime removed and host hardened

- #12–#14, #19 and #20 complete;
- no CLN/LNbits secrets/processes required;
- nginx/systemd/WireGuard/UFW rules installed;
- SSH key-only/root-login-disabled posture applied;
- production runtime account created;
- deploy account broad sudo revoked/narrowed before final secrets;
- domain/DNS/TLS hardening status documented;
- deployed binary provenance/hash recorded;
- operational Strike balance ceiling/sweep procedure documented.

### M6 — Canary accepted

- #15 complete and `docs/testing/phase1-verification-matrix.md` recorded;
- all six addresses resolve and unknown users fail closed;
- tiny real Strike payment succeeds on staging path;
- exactly-once ledger event and address metadata verified;
- Nostr + overlay verified;
- harmless/simulated feeder gateway path verified;
- controlled physical feeder test verified with duplicate UUID replay protection;
- negative trusted-network reachability tests pass.

### M7 — Production cutover

Execute #16 only after operator approval.

## Required verification commands

For Rust changes:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
```

Also run the repository security/audit workflow documented in `docs/implementation-status.md` and the explicit matrix in `docs/testing/phase1-verification-matrix.md`.

## Stop conditions

Codex must stop and require operator action before:

- changing production DNS;
- changing existing clients to the new WireGuard hub;
- disabling the old production VPS;
- installing/rotating final production secrets if broad temporary sudo is still present;
- releasing the local OpenHAB remote-enable/`FeederOverride` gates for a real physical feed;
- granting any Strike spend authority;
- changing the operator-defined maximum online Strike balance/sweep policy.

These are production or physical-safety decisions, not autonomous implementation steps.
