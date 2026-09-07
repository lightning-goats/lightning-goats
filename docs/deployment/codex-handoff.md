# Codex deployment handoff — Phase 1

Status: deployment handoff for the standalone Strike-backed Lightning Goats architecture.

Tracker: https://github.com/lightning-goats/lightning-goats/issues/6

This document is the first operational document to use when Codex is installed on the new VPS. `AGENTS.md` and the linked canonical docs remain authoritative for security invariants and detailed acceptance criteria.

## Mission

Deploy and stage the reviewed Phase 1 software on a new VPS in parallel with the existing production VPS, integrate it with the trusted in-house gateway at `10.8.0.6`, migrate the public `lightning-goats.com` site, and prepare for an operator-gated DNS/WireGuard cutover.

Do **not** redesign the architecture during deployment. If the live environment differs from a documented assumption, record the evidence and stop at the relevant operator gate rather than weakening a security boundary.

## Final Phase 1 architecture

```text
Internet
   |
   v
new VPS
  nginx/TLS + static lightning-goats.com
       |
       +--> 127.0.0.1:8787 lightning-goatsd
       |       |- Strike receive/read only
       |       |- native LNURL/Lightning Addresses
       |       |- durable SQLite ledger
       |       |- Nostr NIP-46 client
       |       |- overlay websocket
       |       `- GatewayClient only
       |
       `--> WireGuard 10.8.0.0/24
                 |
                 v
             10.8.0.6:8789
             lightning-goats-gateway
                 |- dedicated OpenHAB USER token
                 |- correlated feeder-owner request/result
                 |- local feeder rate/safety history
                 `- localhost weather read
```

There is no production LNbits, CLN/Core Lightning, CLNRest, `clnaddress`, OpenHAB token, or spend-capable Strike key on the new VPS.

## Required read order

Before making host changes read:

1. `AGENTS.md`
2. `docs/README.md`
3. `docs/implementation-status.md`
4. `docs/architecture/phase1-strike-architecture.md`
5. `docs/architecture/lightning-address-registry.md`
6. `docs/security/phase1-threat-model.md`
7. `docs/security/openhab-feeder-gateway.md`
8. `docs/security/phase1-hardening-checklist.md`
9. `docs/deployment/wireguard-topology.md`
10. `docs/deployment/new-vps-staging.md`
11. `docs/deployment/public-site-migration.md`
12. `docs/testing/phase1-verification-matrix.md`
13. `docs/deployment/production-cutover.md`

Treat old CLN/LNbits migration documents as historical only.

## Deployment inputs to resolve

Record these in a private deployment log. Never commit secrets.

| Input | Source / rule |
| --- | --- |
| New VPS public IPv4/IPv6 | Vultr instance |
| Temporary staging WireGuard IP | inventory `10.8.0.0/24`; must be unused and not `10.8.0.1` |
| New VPS WireGuard keypair | generate new; never reuse old key during parallel staging |
| Staging HTTPS hostname | operator-approved temporary hostname |
| Strike sandbox receive/read key | sandbox; only required receive/read scopes |
| Strike production receive/read key | production; only required receive/read scopes |
| Strike webhook secret | dedicated webhook secret |
| Temporary webhook-management authority | separate key/credential; revoke after webhook setup |
| Nostr bunker pubkey/client credential/relays | existing NIP-46 deployment; no nsec on VPS |
| OpenHAB Lightning Goats USER token | dedicated token; stored only on `10.8.0.6` gateway |
| Existing feeder result Item | inspect live correlated feeder owner on `10.8.0.6` |
| Existing feeder request payload shape | inspect deployed `feeder-owner.js`; configure exact `{request_id}` template |
| `LightningGoatsRemoteEnabled` | create/verify trusted-side Switch Item |
| Harmless canary request/ack/remote-enable Items | create/verify on OpenHAB; no hardware side effect |
| Operator Nostr contact pubkey | operator supplies; never guess |
| Current website source/assets | copy authoritative files from old VPS; do not scrape rendered page |
| Strike operational balance ceiling | operator chooses before production |

## Account and privilege model

### New VPS

Use separate identities:

- deployment/Codex account, e.g. `lg-deploy`: interactive, temporary sudo during staging;
- production runtime `lightning-goats`: non-admin, preferably non-login;
- nginx/system services use their normal restricted identities.

`lightning-goatsd` is a **system-level** systemd unit with `User=lightning-goats`.

### Trusted host `10.8.0.6`

Run `lightning-goats-gateway` as a separate non-admin OS identity such as `lightning-goats-gateway`. Its OpenHAB API token is injected through the gateway systemd credential and never copied to Vultr.

### Privilege freeze

Broad deployment sudo is a staging capability, not a production feature. Capture all privileged changes in reviewed/reproducible configuration. Before final production secrets/cutover:

1. complete host/config audit;
2. optionally rebuild/reimage from reviewed artifacts for maximum assurance;
3. remove or narrowly restrict broad Codex/deploy sudo;
4. verify root ownership/non-writability of binaries, configs, units, and static site;
5. only then install/rotate final production credentials.

## Actions Codex may perform autonomously during staging

Provided no operator gate below is crossed, Codex may:

- install required OS packages;
- create non-admin runtime/service accounts;
- clone/fetch this repository and build locked Rust binaries;
- install staging binaries/configs/systemd units;
- create the new VPS WireGuard keypair and configure the **temporary** staging peer after confirming the chosen `10.8.0.x` is unused;
- configure staging nginx/TLS under the temporary hostname;
- deploy the trusted gateway and harmless gateway canary on `10.8.0.6` if authorized host access is available;
- inspect the existing OpenHAB feeder-owner rule/configuration read-only;
- create the project-specific OpenHAB integration Items/canary rule when explicitly within the approved Phase 1 scope, without altering the physical feeder owner semantics;
- apply reviewed source-specific UFW rules after preserving existing management access;
- run non-physical unit/integration/security/preflight tests;
- migrate the authoritative static website source from the old VPS into `web/` and apply issue #26 modifications;
- use Strike sandbox and harmless OpenHAB canary paths;
- prepare but not execute production DNS/WireGuard cutover changes.

## Operator gates — do not cross without explicit direction

Codex must stop and request/record operator approval before:

1. stopping or disabling the old production WireGuard hub;
2. assigning `10.8.0.1` to the new VPS;
3. repointing production WireGuard clients to the new VPS public key/Internet endpoint;
4. changing production `lightning-goats.com` DNS;
5. replacing/removing the old production nginx/LNbits/feeder services;
6. enabling a real physical feeder request from the new stack;
7. changing the operator-selected feeder safety limits or Strike balance ceiling;
8. installing a spend-capable Strike credential anywhere in this Phase 1 stack;
9. destroying the old VPS or deleting its backups/CLN recovery material.

## Stage A — build and verify software

On the deployment checkout:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
cargo tree -i rsa --locked
cargo audit --ignore RUSTSEC-2023-0071
cargo build --locked --release --bins
```

Confirm the production binaries include:

```text
lightning-goatsd
lightning-goatsctl
lightning-goats-gateway
```

Record SHA-256 hashes before installation.

Do a source/config sweep. Production code/config must have no runnable references to:

```text
CLNRest
cln-rune
waitanyinvoice
pay_index
clnaddress
LNbits wallet/payment ingress
OpenHAB token on the VPS
```

Historical migration/docs references are permitted only when clearly historical.

## Stage B — new VPS baseline

1. Patch the OS.
2. Install nginx, WireGuard, certificate tooling, build/runtime dependencies, and operational tools required by the repo runbooks.
3. Create `lg-deploy` (or chosen deploy account) and `lightning-goats` runtime account.
4. Disable SSH password authentication and direct root login; verify key-based recovery before tightening network access.
5. Create new WireGuard keypair.
6. Inventory `10.8.0.0/24`; select an unused temporary staging address.
7. Add the new VPS as a new peer without changing the old `10.8.0.1` production hub.
8. Verify it can reach only the intended trusted gateway capability once UFW is in place.

Do not clone the old VPS image wholesale. Rebuild the minimal edge from reviewed artifacts.

## Stage C — trusted gateway on `10.8.0.6`

Use:

- `deploy/gateway/config.toml.example`
- `deploy/gateway/config.canary.toml.example`
- `deploy/systemd/lightning-goats-gateway.service`
- `deploy/systemd/lightning-goats-gateway-canary.service`
- `deploy/ufw/lightning-goats-gateway.sh.example`

### Inspect the existing physical feeder owner

The live OpenHAB feeder owner is known to use rule ID `88bd9ec4de` and `GoatFeeder_ManualRequest`, but **do not assume the current repository snapshot equals the live host**.

Read-only inspect the deployed rule/script and Items to determine:

- exact request command body expected by `GoatFeeder_ManualRequest`;
- exact correlated result Item;
- exact result JSON shape/status values;
- current duplicate/correlation behavior and local physical safety checks.

Set gateway production config:

```toml
request_item = "GoatFeeder_ManualRequest"
ack_item = "<LIVE_RESULT_ITEM>"
request_payload_template = "<EXACT_TEMPLATE_CONTAINING_ONE_{request_id}>"
```

The gateway accepts either an exact UUID ack or a JSON result with matching UUID and explicit successful/completed outcome. Unknown shapes fail closed.

Create/verify `LightningGoatsRemoteEnabled` as a local administrative kill switch. Keep it OFF until the operator-approved physical canary.

### Canary

Deploy the separate port-8790 gateway with harmless request/ack Items. The canary rule may echo the UUID and increment a test counter, but must never actuate feeder hardware.

### Firewall

Review the UFW script before applying it. Do not reset UFW or remove pre-existing management rules blindly. During staging allow only the temporary VPS source to 8789/8790. Direct VPS access to 5000, 8080, 22, 5432 and unrelated trusted-network hosts must fail.

Run:

```sh
deploy/checks/gateway-preflight.sh
```

This script is read-only with respect to the feeder.

## Stage D — `lightning-goatsd` canary on new VPS

Install:

- `/usr/local/bin/lightning-goatsd`
- `/usr/local/bin/lightning-goatsctl`
- `deploy/config.canary.toml.example` -> reviewed `/etc/lightning-goats/config.canary.toml`
- canary systemd unit
- Strike sandbox receive/read credentials
- staging nginx configuration

Use only the port-8790 harmless gateway.

Verify:

- all six Lightning Address discovery paths;
- exact metadata/descriptionHash behavior;
- invoice creation through Strike sandbox;
- invalid/unknown users generate no provider request;
- duplicate settlement notifications remain idempotent;
- feeder accounting `2340 -> two canary feeds -> 340 remainder`;
- canary requests echo the same UUID and do not actuate hardware;
- payment/feed overlay messages use the Phase 1 templates;
- interface/weather messages are overlay-only;
- no Nostr publication occurs in canary mode.

## Stage E — migrate the public website

Copy the **authoritative current source** from the old VPS into `web/` in the repository/worktree before modifying it. Preserve branding/assets/live stream/Nostr chat where independent of retired services.

Apply issue #26:

- remove NIP-05 Verify UI/modal/iframe;
- remove all LNbits/NIP-05/CyberHerd legacy requests;
- hide/disable Phase-2-only leaderboard;
- replace email contact with operator-confirmed Nostr DM/pubkey contact;
- never request raw nsec;
- retain zap controls only if the new invoice path is proven independent of LNbits/CLN;
- use native Lightning Address/LNURL routes;
- ensure no browser-visible secrets.

Serve staging site from nginx and use browser network inspection to prove no retired endpoint calls remain.

Static files deployed to `/var/www/lightning-goats` should be root-owned and non-writable by runtime/deployment users after staging edits are complete.

## Stage F — production-shaped staging

Install production-shaped config/unit but initially keep:

```toml
[service]
mode = "shadow"
```

Use production-shaped gateway port 8789 with `LightningGoatsRemoteEnabled=OFF` and/or `FeederOverride=ON` during non-physical validation.

Configure production Strike receive/read credentials only after privilege/security review. Configure the production webhook with a separate temporary management credential, then revoke that management credential.

Verify native addresses:

```text
herd@lightning-goats.com
dexter@lightning-goats.com
rowan@lightning-goats.com
cosmo@lightning-goats.com
newton@lightning-goats.com
nova@lightning-goats.com
```

Run on the VPS:

```sh
deploy/checks/vps-preflight.sh
```

This preflight intentionally performs no physical feeder action.

## Stage G — required evidence before cutover approval

Produce a deployment report containing at least:

- OS/version and package baseline;
- git commit/PR chain deployed;
- binary SHA-256 hashes;
- systemd unit verification results;
- Rust CI/security results;
- new VPS WireGuard public key and temporary IP (private key excluded);
- UFW effective rules relevant to 10.8.0.6;
- positive gateway reachability and negative 5000/8080/22/5432 tests;
- OpenHAB feeder-owner request/result contract discovered live;
- confirmation `LightningGoatsRemoteEnabled` exists and current state;
- harmless canary test results including duplicate UUID replay;
- Strike sandbox/native LNURL test results;
- Nostr/overlay results;
- weather result;
- migrated site browser-network audit;
- `vps-preflight.sh` and `gateway-preflight.sh` output;
- unresolved operator inputs/gates.

Do not recommend cutover while any mandatory verification item is unresolved.

## Stage H — production cutover

Follow `docs/deployment/production-cutover.md`; do not improvise.

High-level operator-gated sequence:

1. Make physical feeding safe (`FeederOverride`/remote-enable state per runbook).
2. Stop legacy payment/feeder writers so there is one authority.
3. Final backups/archive old VPS state.
4. Stop old VPS WireGuard before new VPS can claim `10.8.0.1`.
5. Assign production hub identity/address and update client peer public key/endpoint as documented.
6. Replace staging UFW allowance with production source `10.8.0.1 -> 10.8.0.6:8789`.
7. Change production DNS.
8. Verify static site + all six Lightning Addresses.
9. Perform one tiny real Strike receive and verify exactly one durable credit, one expected Nostr message, and overlay update.
10. Only with explicit operator approval, enable a controlled single physical feeder canary and verify authoritative correlated result plus no duplicate on UUID replay.
11. Enter observation period; leave old VPS intact for rollback.

## Rollback boundary

Before the first successful real payment/physical action on the new stack, rollback is primarily DNS/WireGuard/service routing.

After the new system has accepted real payments, do not restore the old feeder/payment writer blindly. Preserve exactly-once accounting and follow the production rollback runbook, reconciling any settled payments/feed attempts first.

## Definition of deployment handoff complete

The repository is ready for Codex deployment when:

- Phase 1 code PRs are green and reviewed;
- production code has no CLN/LNbits runtime path;
- gateway and daemon units/configs are present;
- nginx/UFW examples are present;
- preflight scripts are present;
- this handoff and canonical docs agree;
- only environment-specific values/live inspection remain.

Deployment itself is ready for operator cutover approval when the Stage G evidence package is complete and all mandatory checks pass.
