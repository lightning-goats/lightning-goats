# Codex deployment handoff — Phase 1 standalone Lightning Goats

Status: deployment handoff document for the Strike-backed Phase 1 architecture.

Tracker: https://github.com/lightning-goats/lightning-goats/issues/6

This document is the primary execution entry point once the new VPS is provisioned. Read `AGENTS.md` and the linked canonical docs before making changes.

## Objective

Deploy and stage the standalone Lightning Goats architecture in parallel with the current production VPS, then prepare an operator-gated cutover.

Target architecture:

```text
Internet
  |
  v
new Vultr VPS
  nginx
  static lightning-goats.com site
  lightning-goatsd (loopback only)
  WireGuard peer/hub candidate
      |
      | 10.8.0.0/24, narrowly firewalled
      v
10.8.0.6 trusted house host
  lightning-goats-gateway
      |-- dedicated OpenHAB USER token -> loopback OpenHAB REST
      `-- localhost weather read -> 127.0.0.1:5000/get_received_data
```

Payments are Strike-backed. Core Lightning, CLNRest, `clnaddress`, and LNbits are not part of the new production runtime.

## Authority and operator gates

Codex may autonomously:

- inspect repository files and current host state;
- install packages and reviewed repository artifacts while temporary sudo is authorized;
- build/test binaries;
- create non-production users/directories;
- create the new VPS WireGuard keypair and temporary peer configuration;
- configure staging nginx/systemd/firewall rules;
- create harmless OpenHAB canary Items/rule after inspecting the live OpenHAB version and existing conventions;
- inspect the existing live correlated feeder owner and weather service read-only;
- stage the public site on a temporary hostname;
- run non-physical canary tests and negative network tests;
- collect evidence and update implementation/deployment status docs.

Codex must stop for explicit operator approval before:

- changing production DNS;
- stopping or replacing the old `10.8.0.1` WireGuard hub;
- assigning `10.8.0.1` to the new VPS;
- enabling the physical feeder path / `LightningGoatsRemoteEnabled`;
- performing a real physical feeder activation;
- creating/replacing production Strike credentials or webhook subscriptions if doing so changes the live payment path;
- destroying the old VPS;
- making unrelated changes to OpenHAB, household networking, or other services.

Never infer approval from earlier planning language. Record each production gate and wait for the operator.

## Required operator-supplied or live-discovered values

Do not guess these values.

### New VPS

- public IPv4/IPv6 as applicable;
- temporary staging WireGuard address: one verified-unused `10.8.0.x` address;
- temporary staging hostname;
- final production DNS records/TTL state.

### Strike

- production receive-only API key with only the documented receive-request create/read permissions;
- production webhook secret;
- sandbox equivalents for canary where available;
- operator-defined maximum operational Strike balance and sweep procedure.

Runtime `lightning-goatsd` must not receive outbound/spend/withdraw authority.

### Nostr

- production bunker public key;
- NIP-46 client credential/key material required by the existing `nak` flow;
- production relay list;
- public project/operator Nostr contact pubkey for the website.

Never request or place a raw production nsec in browser-visible code.

### OpenHAB on 10.8.0.6

- create/confirm a dedicated OpenHAB USER for the Lightning Goats gateway;
- create a new dedicated API token for that USER;
- store it only as the `openhab-token` systemd credential for the trusted gateway;
- confirm current OpenHAB version and authentication behavior;
- inspect the live feeder owner rule `88bd9ec4de` and deployed source before changing anything;
- confirm `GoatFeeder_ManualRequest` is still the correlated request Item;
- discover the exact persisted correlated result Item used by the live owner;
- discover the exact request command payload accepted by `GoatFeeder_ManualRequest`;
- set `deploy/gateway/config.toml.example` values accordingly;
- create/confirm the new `LightningGoatsRemoteEnabled` administrative kill-switch Item;
- create separate harmless canary request/ack/remote-enable Items and a no-actuation echo/counter rule.

The gateway supports either a simple UUID result or correlated JSON with an explicit request UUID and successful outcome. The production request payload is configurable through a single `{request_id}` template. Do not modify the physical owner merely to fit an assumed payload if its existing correlated contract can be reused safely.

### Public site

Copy the authoritative current `lightning-goats.com` source and assets from the old VPS into `web/` before modifying them. Do not reconstruct from screenshots or rendered HTML when the original files are available.

Determine:

- current nginx document root;
- current `index.html` and static asset paths;
- current live-stream/chat configuration that is independent of LNbits;
- any browser calls to LNbits/NIP-05/CyberHerd endpoints.

Then implement issue #26:

- remove NIP-05 verification UI and calls;
- remove LNbits-specific payment URLs;
- temporarily hide/disable the Phase-2 CyberHerd leaderboard without legacy backend calls;
- preserve live stream and Nostr live chat where independent of LNbits;
- preserve browser-extension Nostr login and never request an nsec;
- use operator-confirmed Nostr pubkey/DM as contact;
- use native Lightning Address/LNURL routes;
- retain zap UI only after its non-LNbits invoice path is verified.

## Unix identities

Do not use the production runtime account as the Codex/admin account.

Recommended:

```text
lg-deploy
  interactive development/deployment identity
  Codex runs here
  temporary sudo during provisioning

lightning-goats
  non-admin production VPS runtime identity
  no interactive login preferred

lightning-goats-gateway
  non-admin trusted-house gateway runtime identity
  no interactive login preferred
```

After staging is complete and before final production credentials/cutover:

1. remove/narrow broad sudo from `lg-deploy`;
2. audit privileged changes and root-owned deployment artifacts;
3. rotate/install final production secrets after the privilege boundary is frozen.

For maximum assurance, a clean reprovision from reviewed repo artifacts before installing final production secrets is acceptable if practical.

## Build and verification

Use Rust 1.88 as pinned by the repository workflow.

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
cargo tree -i rsa --locked
cargo audit --ignore RUSTSEC-2023-0071
```

The `rsa` advisory exception is allowed only while `rsa` is unreachable from the active application dependency graph, as documented by the security workflow. If it becomes reachable, stop.

Build at minimum:

```sh
cargo build --release --locked --bin lightning-goatsd
cargo build --release --locked --bin lightning-goats-gateway
cargo build --release --locked --bin lightning-goatsctl
```

Record SHA-256 hashes for deployed binaries and the source commit.

## New VPS staging sequence

1. Provision the new VPS; patch the OS.
2. Create `lg-deploy` and install Codex under that account.
3. Clone this repository and read `AGENTS.md` + this document.
4. Install build/runtime prerequisites from reviewed package lists.
5. Create a fresh WireGuard keypair for the new VPS.
6. Inventory `10.8.0.0/24` and select an unused temporary `10.8.0.x`; never use `10.8.0.1` while the old VPS is active.
7. Add the new VPS as an additional staging peer without disrupting the current production hub.
8. Create the `lightning-goats` runtime account and root-owned deployment directories.
9. Build/install the reviewed binaries.
10. Install staging config from `deploy/config.canary.toml.example`, resolving only documented placeholders.
11. Install the system-level canary unit.
12. Configure nginx from the repo example and a temporary HTTPS staging hostname.
13. Configure Strike sandbox/canary credentials as systemd credentials.
14. Do not give the VPS any OpenHAB API token.
15. Verify the six Lightning Address discovery endpoints and callback validation.
16. Verify unknown users fail before provider contact.
17. Verify Strike webhook signature/reconciliation behavior with sandbox/mocked evidence.

## Trusted 10.8.0.6 gateway staging sequence

1. Inspect current OpenHAB/version/feeder owner/weather services read-only first.
2. Create the dedicated OpenHAB Lightning Goats USER/token.
3. Create `lightning-goats-gateway` OS runtime identity.
4. Install `lightning-goats-gateway` binary and systemd units.
5. Resolve the production feeder owner request/result contract but do not enable the production gateway for physical actuation yet.
6. Create harmless canary request/ack/remote-enable Items/rule.
7. Configure canary gateway on `10.8.0.6:8790`.
8. Configure production gateway on `10.8.0.6:8789` with remote feeding disabled.
9. Install UFW rules from `deploy/ufw/lightning-goats-gateway.sh.example` only after substituting the verified staging VPS `10.8.0.x`.
10. Verify from the VPS that only the gateway port is reachable on the trusted host.
11. Explicitly verify direct VPS access fails to `10.8.0.6:5000`, OpenHAB REST/admin, SSH (unless separately approved), PostgreSQL, and unrelated LAN services.
12. Verify `/v1/weather` works and there is no gateway route to the legacy `/weather` mutation endpoint.
13. Run canary UUID replay tests and prove one request UUID cannot produce multiple canary actions.
14. Test gateway timeout/ambiguous behavior and later acknowledgement reconciliation.

## Informational/overlay verification

Default intended behavior:

```text
interval:                 60 seconds
interface-info chance:    40% unconditional per interval
weather chance:           40% unconditional per interval
maximum info messages:    one per interval
```

Verify:

- payment messages use `sats_received` templates and reach Nostr + overlay;
- confirmed feeds use `feeder_trigger` templates and reach Nostr + overlay;
- interface-info is overlay-only;
- weather is overlay-only;
- individual goat Lightning Address payments identify that goat in presentation;
- Nostr uses goat Nostr profile references while overlay uses human names/images;
- reconnect/replay does not duplicate payment/feed accounting or physical actions.

## Public-site staging verification

Before production DNS cutover, browser/network inspection on the staging site must show:

- no NIP-05 verification calls;
- no LNbits backend calls;
- no legacy CyberHerd leaderboard calls in Phase 1;
- no secret material in HTML/JS;
- native same-origin Lightning Address discovery/callback operation;
- live stream/chat functional where retained;
- contact is Nostr-only with the operator-confirmed public identity.

Static files must be root-owned and not writable by `lightning-goats` or `lg-deploy` after deployment finalization.

## Production preflight gate

Do not cut over until all of the following are true:

- CI and Security are green for the exact deployment commit;
- `lightning-goatsd` has zero CLN/LNbits runtime dependency;
- no OpenHAB token exists on the VPS;
- gateway canary is fully verified;
- production gateway is configured but remote physical feeding remains disabled;
- UFW/WireGuard positive and negative tests pass;
- all six Lightning Addresses work on staging;
- one small real/sandbox Strike receive path has been reconciled exactly once as appropriate to the test environment;
- Nostr/overlay behavior is verified;
- public site is staged and legacy browser calls are absent;
- rollback to the old VPS remains possible;
- final secrets are installed only after broad deployment sudo is revoked/narrowed.

## Operator-gated cutover outline

Only after explicit operator approval:

1. enable `FeederOverride`/remote-safety state appropriate for a controlled cutover window;
2. stop legacy LNbits/goat-feeder services so there is one authoritative payment/feeder path;
3. stop old VPS WireGuard before the new VPS ever claims `10.8.0.1`;
4. update client peer public key and Internet endpoint to the new VPS while retaining client keys/addresses;
5. make the new VPS the production hub (`10.8.0.1`) if the operator chooses the topology-preserving plan;
6. replace temporary trusted-host UFW source with the production `10.8.0.1` rule;
7. switch production DNS to the new VPS;
8. verify homepage/TLS/LNURL endpoints;
9. make one tiny real Lightning Address payment and verify Strike authoritative settlement, exactly-one durable credit, Nostr message, and overlay message;
10. with a separate explicit physical-test approval, enable the production gateway remote-feed switch and perform one controlled feeder test;
11. replay/query the same UUID and prove no second feed occurs;
12. verify remainder accounting and, if intentionally tested, multi-threshold behavior;
13. return to normal safety state;
14. leave the old VPS intact but inactive during the observation period.

## Rollback boundary

Before the old VPS is destroyed, rollback remains:

- restore old DNS;
- restore old WireGuard hub/client endpoint configuration;
- disable the new production gateway remote-feed switch;
- stop new production services as required;
- restore only the legacy services deliberately chosen by the operator.

Never run old and new feeder/payment authorities concurrently in a way that can double-process the same incoming payment or physical feed.

## Evidence to leave behind

Codex should update `docs/implementation-status.md` and create a dated deployment report containing:

- exact repo commit and binary hashes;
- new VPS OS/version and public/staging addresses (do not record private keys/secrets);
- WireGuard peer/address inventory relevant to the cutover;
- systemd unit status and sandbox summary;
- UFW rules and negative-test results;
- OpenHAB version and the confirmed feeder request/result contract (no API token);
- Strike scopes confirmed for runtime key;
- configured Lightning Address registry;
- staging hostname and browser legacy-call audit;
- canary payment/overlay/Nostr/gateway results;
- unresolved operator gates;
- final go/no-go recommendation.

## Canonical supporting docs

- `docs/planning/phase1-execution-plan.md`
- `docs/architecture/phase1-strike-architecture.md`
- `docs/architecture/lightning-address-registry.md`
- `docs/architecture/phase1-messaging.md`
- `docs/architecture/weather-overlay.md`
- `docs/security/phase1-threat-model.md`
- `docs/security/openhab-feeder-gateway.md`
- `docs/security/phase1-hardening-checklist.md`
- `docs/deployment/wireguard-topology.md`
- `docs/deployment/new-vps-staging.md`
- `docs/deployment/public-site-migration.md`
- `docs/deployment/production-cutover.md`
- `docs/testing/phase1-verification-matrix.md`

When documents conflict, this handoff plus the newest implementation/status docs and actual tested code take precedence over historical CLN/LNbits planning material.