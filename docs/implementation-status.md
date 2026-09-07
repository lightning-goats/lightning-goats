# Phase 1 Implementation Status

Last implementation update: 2026-09-07.

Tracker: https://github.com/lightning-goats/lightning-goats/issues/6

## Current state

Phase 1 has reached a **software/deployment release-candidate** state on the stacked implementation branches. The remaining work is primarily live provisioning, discovery of deployment-specific values, public-site source migration, staging verification, and the operator-gated production cutover.

For deployment, start with:

- `docs/deployment/codex-handoff.md`
- `AGENTS.md`

The approved production architecture is:

- native Lightning Address / LNURL-pay in `lightning-goatsd`;
- configured users `herd`, `dexter`, `rowan`, `cosmo`, `newton`, and `nova`;
- one common `herd` feeder-credit pool with durable `address_user` metadata;
- Strike as the only Lightning/payment backend;
- receive/read-only Strike authority in `lightning-goatsd`;
- no LNbits, Core Lightning, CLNRest, `clnaddress`, CLN rune, or CLN `pay_index` production runtime dependency;
- no OpenHAB token on the public VPS;
- purpose-built trusted `lightning-goats-gateway` on `10.8.0.6` owning the dedicated OpenHAB USER token;
- existing `10.8.0.0/24` WireGuard network with host/UFW containment;
- durable payment/feed accounting and ambiguity-safe physical actuation;
- data-driven payment/feed templates, durable Nostr outbox, and overlay replay;
- interface/weather informational messages overlay-only;
- the public static site served on the new VPS after source migration/removal of NIP-05/LNbits/Phase-2-only backend calls;
- clean Phase 2 seam for future CyberHerd functionality.

## Implemented / in review

### Payment and accounting

- #7 backend-neutral payment/ledger domain: implemented and merged via PR #22.
- #8 receive-only Strike integration and authoritative settlement reconciliation: implemented in mainline PR #24; prior checks were green.
- #9 + #18 native LNURL-pay and six-address registry: implemented in PR #25; prior CI/Security were green and the PR was marked ready for review.

### Messaging and overlay

- #10 + #11 Phase 1 templates and shared Nostr/overlay rendering: implemented in PR #27.
- Template/goat selection is deterministic from durable event identity so retries/restarts render the same presentation.
- Individual goat-address payments use the paid goat in presentation.
- Payment/feed events are Nostr + overlay; `interface_info`/`weather_status` are overlay-only.

### Trusted OpenHAB/weather boundary

- #17 + #21 implemented in PR #28 / branch `phase1/integration-gateway`.
- `lightning-goatsd` uses only `GatewayClient`; it has no OpenHAB token or direct generic OpenHAB access.
- New `lightning-goats-gateway` binary provides the narrow trusted API.
- Gateway persists feeder UUID intents before command issuance and never resends an existing pending UUID.
- Gateway supports exact UUID acknowledgement or explicitly successful correlated JSON result.
- Gateway request payload is a tightly constrained configurable template containing exactly one `{request_id}` placeholder so deployment can bind to the live correlated OpenHAB owner contract without another code change.
- The existing live feeder owner (`88bd9ec4de`, currently triggered by `GoatFeeder_ManualRequest`) is preferred as physical authority; deployment must inspect and bind to its actual live request/result contract rather than guessing.
- Gateway adds persisted minimum-feed interval and rolling feeds-per-hour caps as defense in depth.
- Weather is read only from local `127.0.0.1:5000/get_received_data`, sanitized, and never exposes the legacy mutating `/weather` endpoint.
- 60-second informational evaluation with configurable 40% interface-info and 40% weather unconditional probabilities is implemented with at most one informational event per cycle.
- Production/canary gateway systemd/config/UFW examples are in `deploy/`.

A CI-discovered first-start bug in gateway SQLite initialization was fixed by enabling `create_if_missing` with WAL/FULL synchronous options. Current branch checks must be green for the exact deployment commit before deployment.

### CLN/LNbits removal

- #13 is implemented in clean PR #30 / branch `phase1/remove-cln-final`.
- The clean branch was created from the current trusted-gateway branch; the older divergent development branch containing a temporary write-capable workflow was intentionally **not** merged.
- `src/cln/` and `invoice_watcher` are deleted.
- Strike + LNURL are mandatory configuration.
- CLN cursor/status/operator initialization paths are removed.
- Production/canary config and systemd units have no CLN rune/CA configuration.
- migration `0005_remove_legacy_cln.sql` removes obsolete transition tables after provider-neutral migration.
- dead `clnaddress` label parsing was removed; only generic Lightning Address user validation remains.

### Deployment and edge hardening artifacts

Repository assets now include:

- hardened VPS systemd production/canary units;
- hardened trusted-gateway production/canary units;
- production/canary application configs;
- production/canary trusted-gateway configs;
- nginx production/canary examples;
- explicit nginx method restrictions and loopback-only `/healthz` in the production example;
- LNURL callback/discovery and Strike webhook edge rate limits;
- UFW/WireGuard gateway containment template;
- threat model / hardening checklist / verification matrix;
- parallel-VPS staging and production-cutover runbooks;
- comprehensive `docs/deployment/codex-handoff.md`.

## Work that deliberately remains for deployment

### #26 public website migration

The authoritative current `lightning-goats.com` source and static assets are on the existing VPS and should be copied into this repository (preferred `web/`) during new-VPS staging. Do **not** reconstruct the production site from screenshots/scraped rendering when the originals are available.

Deployment must then:

- preserve the live stream and Nostr live chat where independent of retired services;
- remove NIP-05 verification UI/calls;
- remove LNbits-specific payment URLs;
- hide/disable the live CyberHerd leaderboard during Phase 1 so it makes no legacy backend requests;
- keep browser-extension Nostr auth and never request a raw nsec;
- change contact to operator-confirmed Nostr pubkey/DM;
- retain zap UI only if its invoice path is proven independent of LNbits/CLN;
- deploy the static tree root-owned on the new VPS.

See `docs/deployment/public-site-migration.md` and issue #26.

### Live environment binding on 10.8.0.6

Codex must inspect rather than guess:

- current OpenHAB version/runtime;
- exact correlated feeder result Item for live owner `88bd9ec4de`;
- exact `GoatFeeder_ManualRequest` request payload contract;
- dedicated new OpenHAB USER/API token for the gateway;
- creation/confirmation of `LightningGoatsRemoteEnabled`;
- harmless canary request/ack/remote-enable Items/rule;
- current weather service behavior on localhost port 5000.

### New VPS / WireGuard / secrets

Deployment must resolve:

- new VPS public IP;
- verified-unused temporary `10.8.0.x` staging address;
- new VPS WireGuard keypair;
- staging hostname/TLS;
- Strike sandbox/production receive-only credentials and webhook secret;
- Nostr bunker/client/relay values;
- operator-confirmed public Nostr contact identity;
- operator-defined maximum operational Strike balance/sweep policy.

The old `10.8.0.1` hub remains authoritative during staging. The new VPS must never claim `10.8.0.1` until the old WireGuard service has been explicitly stopped during the operator-approved cutover.

## Remaining Phase 1 gates

- #12: confirm/document the final CyberHerd Phase 2 boundary against the release candidate; architecture docs already establish the intended seam.
- #14/#19/#20: repository hardening controls/artifacts largely exist; verify/apply them on the actual new VPS/domain/Strike account.
- #15: execute the complete end-to-end staging/security verification matrix against real staging infrastructure.
- #16: execute the production cutover/rollback runbook only with explicit operator approval.
- #26: migrate and simplify the actual public website source during staging.

## Deployment method

1. provision the new VPS;
2. create `lg-deploy` and install Codex under that separate deployment identity;
3. read `docs/deployment/codex-handoff.md` first;
4. add the new VPS to `10.8.0.0/24` using a new keypair and verified-unused temporary `10.8.0.x` address;
5. keep old `10.8.0.1` and production DNS authoritative;
6. build/install the release-candidate binaries and staging nginx/systemd configuration;
7. install/configure trusted production + harmless canary gateway instances on `10.8.0.6`;
8. create/store the dedicated OpenHAB gateway token only on the trusted host;
9. apply reviewed UFW containment and prove negative connectivity from the VPS;
10. copy the authoritative website source/assets from the old VPS into `web/`, modify per #26, and stage it;
11. run all automated + staging verification;
12. revoke/narrow broad deployment sudo and audit permissions before final production secrets/cutover;
13. with explicit operator approval, stop old hub WireGuard, move/update the new hub/client configuration, switch DNS, perform a tiny payment test, then separately approve a controlled physical feeder test;
14. leave the old VPS inactive but intact during observation/rollback period.

## Phase 1 Lightning Address scope

```text
herd@lightning-goats.com
dexter@lightning-goats.com
rowan@lightning-goats.com
cosmo@lightning-goats.com
newton@lightning-goats.com
nova@lightning-goats.com
```

All six use `credit_pool=herd` while preserving `address_user`. Nginx may forward generic paths, but the application registry is authoritative and unknown users fail before provider contact.

## Required Rust/security gate

Run against the **exact candidate commit**:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
cargo tree -i rsa --locked
cargo audit --ignore RUSTSEC-2023-0071
```

`RUSTSEC-2023-0071` may remain ignored only while `rsa` is unreachable from the active application dependency graph. If it becomes reachable, stop and remediate.

## Definition of deployment-ready

The repository is ready to hand to Codex for deployment when the current CLN-free release-candidate branch has green CI + Security. The deployment itself is not complete until:

- actual website source is migrated/staged;
- all six Lightning Addresses pass staging;
- Strike settlement is reconciled exactly once;
- Nostr/overlay behavior is verified;
- trusted gateway canary + weather + UUID replay/ambiguity tests pass;
- VPS cannot directly reach OpenHAB/weather/SSH/Postgres/unrelated LAN paths;
- final SSH/domain/DNS/credential controls are applied;
- operator approves and executes the production WireGuard/DNS cutover;
- one tiny real payment and one separately approved controlled physical feed pass;
- the rollback observation period succeeds.

## Phase 2 boundary

CyberHerd membership, headbutts, rewards/distributions, NIP-05, and outbound Lightning spending remain outside Phase 1. Future CyberHerd can be a separate `cyberherdd` service or an internal module, but it must consume the same durable payment/feeder/event boundaries and must not acquire a direct OpenHAB bypass. Any future spend-capable Strike credential should remain in a separately privileged payout component.
