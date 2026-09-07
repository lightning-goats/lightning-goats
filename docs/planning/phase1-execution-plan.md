# Phase 1 Execution Plan — Strike-backed Lightning Goats

Status: approved planning context; execution has not started.

Tracker: https://github.com/lightning-goats/lightning-goats/issues/6

## Objective

Replace the production LNbits/Core Lightning path with a small standalone `lightning-goatsd` stack on a new VPS while preserving:

- `herd@lightning-goats.com` Lightning Address payments;
- individual goat Lightning Addresses: `dexter`, `rowan`, `cosmo`, `newton`, `nova`;
- Strike-backed Lightning receive requests;
- durable exactly-once feed-credit accounting;
- the existing feeder and `FeederOverride` safety behavior, strengthened by an in-house integration gateway and local OpenHAB command/ack safeguards;
- payment-received and feeder-triggered Nostr messages;
- the production video overlay;
- informational/interface/weather messages on the overlay only;
- the existing weather message behavior sourced from the in-house weather receiver.

Phase 1 intentionally excludes CyberHerd membership, headbutts, NIP-05 verification, rewards/distributions, and spend-capable Lightning/Strike authority.

## Approved deployment strategy

Build the replacement stack on a **new VPS in parallel** with the current production VPS. The old VPS stays authoritative until the replacement passes the full verification matrix.

Reuse the established WireGuard network:

```text
10.8.0.0/24
```

Known nodes:

```text
10.8.0.1   existing production VPS / WireGuard hub
10.8.0.6   in-house OpenHAB + weather host
```

The new VPS receives its own WireGuard keypair and an unused temporary `10.8.0.x` address during staging. Existing WireGuard clients continue using the old VPS while testing proceeds.

Do not reuse the old VPS WireGuard private key and do not assign `10.8.0.1` to the new VPS while both hosts are online.

Preferred production cutover preserves `10.8.0.1` as the hub address: stop old WireGuard first, then move the reviewed hub configuration/address to the new VPS while clients update the hub public key and Internet endpoint. See `docs/deployment/wireguard-topology.md`.

The VPS must not have generic OpenHAB/LAN access. The only normal application path to `10.8.0.6` is the dedicated in-house Lightning Goats integration gateway port.

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

It must **not** receive an OpenHAB token. OpenHAB credentials live only on the trusted in-house integration gateway.

Do not run Codex as the production runtime identity.

## Recommended work order

The GitHub issues are the executable work queue.

1. #7 — backend-neutral payment/ledger domain.
2. #8 — receive-only Strike API integration and settlement reconciliation.
3. #9 — native Lightning Address/LNURL-pay endpoints.
4. #18 — configured registry for herd + five individual goat Lightning Addresses.
5. #10 — Phase 1 message templates.
6. #11 — templated durable events to Nostr + overlay.
7. #21 — preserve weather overlay behavior through the sanitized in-house gateway path.
8. #12 — CyberHerd-ready service/event boundaries.
9. #17 — in-house OpenHAB/weather integration gateway + WireGuard/UFW boundary.
10. #13 — remove CLN/LNbits/clnaddress runtime assumptions.
11. #14 — VPS/nginx/WireGuard/credential hardening.
12. #19 — domain/DNS, SSH, deployment provenance and operational Strike balance hardening.
13. #20 — LNURL/webhook abuse controls.
14. #15 — full end-to-end verification matrix.
15. #16 — production cutover and rollback runbook.

Issues #10, #12, #17, #19, #21 and much of #14 may proceed in parallel with the payment work. #18 follows the generic LNURL work in #9. #16 is gated on #15 and the completion/operator acceptance of the security-boundary issues.

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
- The VPS cannot connect directly to the legacy weather receiver on `10.8.0.6:5000`; weather is obtained only through sanitized gateway `/v1/weather`.
- Weather is presentation-only and never enters the Nostr outbox.
- Presentation failures never roll back or duplicate financial/feed state.
- Payment and feeder messages go to Nostr + overlay.
- Informational/interface/weather messages go to overlay only.
- Nostr signing remains isolated through `nak`/NIP-46.
- Public LNURL callback and webhook ingress is method/body/rate constrained.
- No production LNbits, CLNRest, Core Lightning, `clnaddress`, or CLN `pay_index` dependency remains after cutover.

## Weather compatibility

Behavioral references:

```text
lightning-goats/middlware/weather.py
lightning-goats/lightning_goats_extension/services/weather.py
lightning-goats/lightning_goats_extension/services/messaging.py
lightning-goats/lightning_goats_extension/tasks.py
lightning-goats/lightning_goats_extension/config.py
```

Current weather read source:

```text
http://10.8.0.6:5000/get_received_data
```

The gateway on/trusted to `10.8.0.6` reads this locally and exposes only normalized `/v1/weather` to the VPS.

Current code defaults are the migration baseline:

```text
informational evaluation interval = 60 seconds
weather broadcast probability     = 0.40 per interval
```

When interface-info and weather are both enabled, current behavior evaluates interface info first, emits at most one informational message per cycle, and adjusts the conditional weather draw so weather retains its 40% unconditional chance. See `docs/architecture/weather-overlay.md`.

## Staging milestones

### M0 — New host bootstrapped

- new VPS provisioned;
- deploy/Codex account created;
- temporary sudo enabled;
- Codex and Rust toolchain installed;
- repo cloned;
- live `10.8.0.0/24` peer/address inventory captured;
- new VPS added to the existing WireGuard network with its own keypair and unused temporary `10.8.0.x` address;
- final future hub config for `10.8.0.1` prepared separately but not activated;
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

- #10, #11 and #21 merged;
- fun goat-fact payment messages work;
- feeder-trigger messages work;
- Nostr durable outbox works;
- overlay reconnect/replay works;
- informational messages are overlay-only;
- sanitized weather messages reproduce the existing Lightning Goats extension style and 60-second/40% default scheduling behavior;
- direct VPS access to `10.8.0.6:5000` remains blocked.

### M4 — In-house security boundary established

- #17 complete;
- dedicated OpenHAB integration USER/token exists only on trusted gateway host;
- gateway on `10.8.0.6` (or explicitly approved adjacent host) is the only application ingress from VPS;
- VPS can reach gateway but cannot reach generic OpenHAB REST/admin, weather port 5000, SSH or unrelated LAN services;
- request UUID/ack path works;
- duplicate UUID does not actuate twice;
- local physical safety gates are authoritative;
- `/v1/weather` is read-only/sanitized and cannot expose legacy `/weather` mutation.

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
- weather overlay path verified through gateway;
- harmless/simulated feeder gateway path verified;
- controlled physical feeder test verified with duplicate UUID replay protection;
- negative trusted-network reachability tests pass.

### M7 — Production cutover

Execute #16 only after operator approval.

Preferred WireGuard portion of cutover:

1. stop old VPS WireGuard and verify `10.8.0.1` is no longer active;
2. activate new hub configuration using `10.8.0.1/24` on new VPS;
3. repoint existing clients to the new VPS public key/public endpoint while preserving client keys/addresses;
4. replace temporary staging UFW allowances on `10.8.0.6` with final production source `10.8.0.1` gateway-only rule;
5. remove temporary staging WireGuard address/rules.

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
- stopping old hub/assigning `10.8.0.1` to the new VPS;
- disabling the old production VPS;
- installing/rotating final production secrets if broad temporary sudo is still present;
- releasing the local OpenHAB remote-enable/`FeederOverride` gates for a real physical feed;
- granting any Strike spend authority;
- changing the operator-defined maximum online Strike balance/sweep policy.

These are production or physical-safety decisions, not autonomous implementation steps.
