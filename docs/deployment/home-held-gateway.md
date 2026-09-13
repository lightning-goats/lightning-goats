# Local held gateway binding

Adds explicitly selected `uuid_held_canary` for the published generation2 pair:
`LightningGoatsHeldCanary2Request` / `LightningGoatsHeldCanary2Ack`. Other pairs,
including the original echo canary, failed held generation1 and physical owner,
are rejected under this protocol. Commands and receipts remain bare UUIDs; public
HTTP request/status semantics are unchanged. Existing v1/v2/uuid_canary branches
remain available with their prior bindings.

The fixture/control contract and actual local hold/release evidence are in PR77
with helper correction `3b16b7a85e0a2ab88b42562c78a8b7dde728513f` awaiting
independent re-review. Its completed Count1 baseline is
preserved. The shared source claim was ACKed in #17 comments5655545033/5655590064;
final generation2 names and preserved failed generation were published in
comments5655574018/5655597421 before this adapter change.

## Inactive local installation

`config.held-canary.toml.example` fixes loopback `127.0.0.1:8791`, a separate
SQLite store, generation2 safety Items and shipped 5-second caps/timeouts. It does
not bind WireGuard or replace the existing echo listener on 127.0.0.1:8790.
`lightning-goats-held-canary.service` uses its own locked Unix user, root-owned
binary/config, separate 0700 state/runtime directories, no capabilities,
read-only system protection and loopback-only IP access.

This harmless instance deliberately reuses the existing **canary** OpenHAB USER
credential via encrypted systemd delivery. Unix identities/state are separate;
OpenHAB principals are not. Coarse USER rights are not per-Item isolation. The
credential remains on HOME and is never emitted by these tools or sent to VPS.

`prepare-held-gateway.py` reuses the reviewed fresh-only HOME installer with only
this new identity and three new files. Default mode renders hashes; apply requires
root, matching binary SHA/ELF, safe root-owned parents, absent identity/resources,
no existing unit enablement and canonical fixed target configuration. It never
starts/enables the service and checks installed bytes/modes/inactive state.

```sh
cargo +1.88.0 build --locked --release --bin lightning-goats-gateway
python3 deploy/scripts/prepare-held-gateway.py \
  --binary target/release/lightning-goats-gateway --sha256 REVIEWED_SHA256
# Only the fresh local inactive installation:
sudo python3 deploy/scripts/prepare-held-gateway.py \
  --binary target/release/lightning-goats-gateway --sha256 REVIEWED_SHA256 --apply
```

After exact-source verification, inspect the installed unit/config/executable,
credential metadata, listening address and fixed fixture before any local command
test. Existing echo/production services and stores are not installation targets.
A separate local test may start only this unit, verify remote-OFF refusal, then
exercise a held request, restart with the same DB, release exactly its UUID via
HOME control and confirm via GET without a second delivery. Return its remote
switch OFF and retain both gateway and fixture histories. Do not reset Count.

## Cross-host and rollback boundaries

This local installation is not network acceptance. A future reviewed binding to
HOME WireGuard TCP 8790 can coexist with the preserved loopback-only echo socket;
it needs the approved peer/containment manifest and a correspondingly reviewed
unit policy. Do not relax this unit's IP policy or change its listener as part of
this installer. VPS receives only the gateway API and source-pinned evidence;
HOME retains direct release control and OpenHAB credentials.

Rollback stops/disables only the new held gateway and preserves its nonempty
SQLite store and all fixture records. Pending UUIDs require exact reconciliation,
not a fresh request or old backup. Original canary and physical-owner configuration
remain unchanged. Source-level binding tests validate allowed/rejected pairs and
bare UUID delivery/receipt matching; real cross-host accounting remains open.

## Installed inactive evidence (2026-09-13)

Source `daeb9ba0acfd4b506547aba8049a3f1a9aae3cc3` passed all 13 CI jobs.
The fresh installer applied successfully; separate `--verify` and
`systemd-analyze verify` passed. The unit is inactive/disabled with MainPID 0,
no listener on 8791, and an empty runtime-owned 0700 state directory. The account
is password-locked, nonzero UID, nologin, with only its primary group. Exact
root-owned artifact hashes are in
[installation evidence](../testing/evidence/home-held-gateway-install-20260913.json).
This verifies installation, not running credential delivery or request acceptance.

The original echo gateway remains active/boot-disabled; production remains
inactive/disabled. No OpenHAB request, fixture mutation, network change or physical
command was made for this installation. PR77 correction review and PR79 source
review remain pending before the next actual local held-gateway acceptance.

## Read-only runtime verification (2026-09-13)

After VPS accepted PR77 correction `3b16b7a`, HOME started only the separate held
unit for GET-only validation. `--verify --running` confirmed exact installed
executable/arguments, expected UID, zero effective capabilities and
NoNewPrivileges. The listener was only `127.0.0.1:8791`. Encrypted credential
loading and real OpenHAB reads returned both canary safety switches false.
`/healthz` returned 200; optional temperature returned 200/null; actual weather
correctly returned 502/unavailable. No feeder POST or switch mutation occurred.

The service was stopped in the check's cleanup and is inactive/disabled with
MainPID0. Its database is now initialized (not an empty directory): integrity
check is ok, with zero request/refusal rows. Preserve this store. Sanitized
[read-only runtime evidence](../testing/evidence/home-held-gateway-readonly-20260913.json)
is separate from the earlier inactive-installation snapshot. PR79 review and
actual held request/restart/release acceptance remain open.
