# Home-server agent handoff: OpenHAB and weather gateway

**Assignment:** complete the home-side Lightning Goats integration on `10.8.0.6`, while the other agent owns the VPS. Start here for home-server work, then read `../../AGENTS.md`. This role-specific execution order takes precedence over the older VPS-first reading order; it does not relax the security invariants or production gates.

**Prepared:** 2026-09-12. **Inspected source:** `bf8749645c6e6942643a3b821cbb8804e7e200a8` on `main`, the PR #56 integration of #31–#55. Recheck current refs and working trees before continuing. The gateway already exists: install, validate, and finish its integration rather than creating a replacement daemon.

**Target:** a tested, least-privilege home gateway and a precise connection contract for the VPS agent. A harmless canary can be completed independently. Physical feeding and production cutover remain on **HOLD** until their separate acceptance gates pass.

## 1. Ownership and parallel-work agreement

| Home agent owns | VPS agent owns | Joint acceptance |
| --- | --- | --- |
| Home inventory; gateway build/install/configuration; dedicated OpenHAB credential; harmless canary Items/rule; local weather validation; home firewall proposal and approved changes | Strike/LNURL; daemon and website; nginx/TLS; Nostr/overlay; VPS credentials; VPS-side tunnel configuration | Pinned source/API contract; staging connectivity; combined harmless canary; final release; operator-approved cutover |

The **home agent is the primary coordinator for the existing physical owner's contract/finality work**, but first check for unpublished or active work by the other agent. Adopt or review that work rather than writing another owner. Changes to `src/openhab.rs`, `src/gateway/`, or the wire protocol require coordination because the VPS client depends on them. Do not alter the VPS, its checkout, DNS, or the old VPN hub from this assignment.

Use a clean branch such as `home/gateway-integration-20260912` from current `origin/main`. Inspect `git status` first; preserve local work, deployed scripts, and independent branches. Do not reset, force-push, delete branches, or replace a deployed checkout. Fetch/rebase or merge reviewed changes normally. The historical owner-v2 branch name is **not** evidence that an implementation was published. Inspect its actual commits.

Post a short ownership/checkpoint comment on issues #17 and #21, referencing #6. Use focused PRs against current `main` for source, tests, scripts, and sanitized evidence. Do not duplicate or individually replay the already-integrated #31–#55 stack. Keep #6/#15/#16 open.

## 2. Authority and stop conditions

Within the operator-granted home-host privileges, proceed with read-only discovery, local code/tests, fresh project runtime accounts/directories, project-only credential provisioning, inactive installation, and isolated harmless canary setup/testing. Inspect existing resources before any write; an existing Item, account, file, or unit is not permission to replace it.

**Separate explicit approval is required before** changing the live physical owner, any physical actuator or production safety Item, global openHAB API-security settings, household routing/firewall policy that changes current access, existing shared services, production gateway activation, physical feeding, or rebooting/upgrading openHAB. DNS, hub replacement, real payments, and VPS cutover are outside this assignment. Prepare the exact proposed change and rollback while waiting; continue independent safe work instead of stopping the whole project.

Never use `/rest/rules/88bd9ec4de/runnow` as a test. Never turn off the household `FeederOverride` to make a canary pass. Never run a production-request POST to test whether a safety gate works. Do not clear pending UUIDs, reset databases, or weaken completion parsing to restore availability.

## 3. Read only the relevant source first

Read these files at the selected commit:

- `src/bin/lightning-goats-gateway.rs`, `src/gateway/server.rs`, `src/gateway/client.rs`, `src/gateway/store.rs`, `src/gateway/weather.rs`, `src/openhab.rs`, `src/secrets.rs`.
- `deploy/gateway/config.toml.example`, `deploy/gateway/config.canary.toml.example`, and both `deploy/systemd/lightning-goats-gateway*.service` units.
- `docs/security/openhab-owner-contract.md`, `docs/security/openhab-jdbc-recovery-evidence.md`, and `docs/security/hardening/owner-finality/hardening.md` with its linked proposal.
- `docs/deployment/audit-remediation.md`, `docs/deployment/staging-acceptance-evidence.md`, and `docs/testing/phase1-verification-matrix.md`.

Older documents contain superseded completion claims and template-based owner instructions. The current source, audit corrections, and source-pinned evidence take precedence. At the inspected baseline, only `feeder_request_v1` and `uuid_canary` are implemented; no owner-v2 completion protocol is established.

## 4. Discover the actual home environment

Known values are starting points, not a substitute for local inspection:

| Item | Known baseline |
| --- | --- |
| Home OpenHAB/weather host | `10.8.0.6` |
| Existing VPN | `10.8.0.0/24`; old hub `10.8.0.1` |
| New VPS staging identity | `10.8.0.12`, previously operator-reserved; confirm actual peer/path with VPS agent |
| Home gateway / harmless canary | TCP `8789` / `8790` respectively |
| OpenHAB origin | `http://127.0.0.1:8080/` if locally verified |
| Weather read endpoint | `http://127.0.0.1:5000/get_received_data` |
| Existing owner | rule `88bd9ec4de` |
| Production request / result | `GoatFeeder_ManualRequest` / `GoatFeeder_ManualResult` |
| Existing override | `FeederOverride` |
| Project production remote-enable | `LightningGoatsRemoteEnabled`, desired default OFF |
| Optional temperature Item | `AmbientWeatherWS2902A_WeatherDataWs2902a_Temperature` |

Record OS/architecture, installed OpenHAB/Java/JS automation/JDBC versions, systemd/credential support, UFW or other actual firewall manager, MAC policy (SELinux/AppArmor), listeners, time synchronization, storage/free space, backup method, gateway services/accounts, and any legacy feeder callers. Do not assume the home's OS matches the Fedora VPS.

Inspect the current owner, request/result Item types, linked metadata, persistence/restore behavior, and every caller before changing a shared contract. The previous exported owner script SHA-256 was `730053e0f3245cb83461e3fe6e3b05d49c8b508631e8cdb4a889c8be8d915978`. Compare, do not overwrite a different live script to match that hash. The historical local export is `/home/sat/lg-owner-review-88bd9ec4de.json`; revalidate its age against the live owner.

The deployed `earthship-ui` checkout may contain unpublished owner work. Inspect its location, branch, status and script provenance without resetting it. Coordinate source ownership with that repository if the owner's maintained source belongs there. Do not edit live JSONDB files to bypass the supported OpenHAB management interfaces.

Keep raw owner exports, topology, private addresses beyond this contract, and account details in operator-controlled storage. Commit only necessary sanitized fixtures, hashes, versions, and conclusions. Never capture `wg showconf`, `wg ... private-key`, or dump-form output containing private/preshared keys. Selectively inventory peer public keys, allowed prefixes, endpoints, interfaces, and routes instead.

## 5. Minimum OpenHAB permission contract

The public VPS receives **no OpenHAB token**. The existing gateway authenticates locally using the token as the HTTP Basic username and an empty password. Its required runtime operations are:

| Method and path | Purpose | Production binding |
| --- | --- | --- |
| `GET /rest/items/<override_item>/state` | Read safety override | `FeederOverride` |
| `GET /rest/items/<remote_enabled_item>/state` | Read local remote-enable gate | `LightningGoatsRemoteEnabled` |
| `GET /rest/items/<ack_item>/state` | Read correlated result | `GoatFeeder_ManualResult` |
| `GET /rest/items/<temperature_item>/state` | Optional temperature | Confirm the configured Item and units |
| `POST /rest/items/<request_item>` with `Content-Type: text/plain` | Submit one typed, UUID-correlated command | `GoatFeeder_ManualRequest`; **no live testing yet** |

No runtime Item/rule creation, rule editing, `/runnow`, direct actuator commands, override/remote-enable writes, general proxy, PostgreSQL credentials, or Strike/Nostr credentials are required. The current adapter does **not** query JDBC history. Any future receipt-recovery read must be explicitly designed, versioned, permission-reviewed and tested, not added as a guessed wildcard permission.

Create a project-specific OpenHAB identity/token using the installed version's supported mechanism. Prefer a dedicated USER-level identity named `lightning_goats_gateway` and token label `lightning-goats-gateway`; verify effective rights rather than relying on the label. Token creation may require administrative provisioning, but the resulting runtime credential must not silently retain admin authority. If the installed version cannot provide the intended USER-only credential, record the limitation and obtain an explicit decision before supplying broader rights; continue mocked/canary preparation independently.

OpenHAB's documented USER role is coarse: it permits interacting with existing Items, not a per-Item allowlist. The table above is the **application's required API surface**, not a claim that the token itself enforces those exact paths. Never manufacture custom Item-scope names and call them authorization. Verify authentication and the lack of admin access through harmless reads and supported account metadata, not destructive negative tests. A missing/invalid-token read can still succeed with Implicit User Role enabled; record that behavior and its impact. Do not globally disable it without an inventory of affected household clients and approval.

Use a separate provisioning identity for creating project Items/rules. Test runtime command permission only against the reviewed harmless canary request. Keep token values out of shell arguments, history, logs, command tracing, Git, screenshots and chat. Use a hidden local input or approved secret-file/credential workflow. Return only token label, verified role, credential path and rotation instructions to the VPS agent.

Store the token using the gateway's systemd credential name **`openhab-token`**. The production unit's example ciphertext path is `/etc/credstore.encrypted/lightning-goats-gateway-openhab`. Encrypt on the home host with its existing protected credential store; never replace that store's key. Verify service-context decryption without printing the value. Rehearse invalid/missing credentials using synthetic copies only. If encrypted credentials are unsupported, propose a documented compatibility change; do not remove the unit directive and expose the token in an environment variable.

## 6. Install the existing gateway, without activating production

Build from an agreed, pinned commit or use its checksum-verified release artifact. Confirm architecture/libc compatibility with the home host. Do not run an unverified downloaded binary as root. The package verifier and install/systemd rehearsal scripts already exist; inspect their `--help`/source and scope before using them. `prepare-vps-canary.py` is a VPS installer, **not** a home-gateway installer. Never run namespace/root rehearsal scripts against the host network to get past their guards.

Minimum source gates, as an unprivileged build user in a clean checkout:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
cargo build --release --locked --bin lightning-goats-gateway
```

Use the repository's Rust 1.88 toolchain and exact-commit Security gate. Provider/owner tests must remain mocks. Do not ignore failing tests or add advisory exemptions. Record source and binary SHA-256 plus relevant CI URLs.

Install `/usr/local/bin/lightning-goats-gateway` root-owned and non-writable by the agent/runtime. Production identity: `lightning-goats-gateway`, locked/non-login, no sudo, no unnecessary supplementary groups. Production config: `/etc/lightning-goats-gateway/config.toml`; state: `/var/lib/lightning-goats-gateway/gateway.db`. Preserve the hardened **system-level** unit, `User=...`, empty capability set, private state/runtime directories and systemd credentials. Do not switch to user-level systemd.

Prepare the production config from the example, retaining `LightningGoatsRemoteEnabled=OFF` and the current owner protocol until reviewed changes are available. The example's 30-second interval and 10-feeds/hour cap require operator sizing approval; do not increase them to pass a test. Stage this unit/config **inactive**, without enabling or starting it or changing an existing running instance. Back up existing project files; use an upgrade plan rather than blindly copying over them.

Inspect the filesystem backing SQLite: it must be durable/local and support the required WAL/FULL behavior. Preserve unresolved UUIDs, refusals, rate history and weather watermark. All instances targeting one physical owner must share the authoritative durable gateway state; do not deploy independent physical gateways with separate databases. Separate stores are correct only for the harmless canary and physical path, which have different targets.

## 7. Build and run a genuinely harmless canary

Create/reuse only reviewed, unlinked project test Items:

- String `LightningGoatsCanaryRequest` and String `LightningGoatsCanaryAck` (these exact names are required by `uuid_canary`).
- Switch `LightningGoatsCanaryRemoteEnabled`, initially OFF.
- Switch `LightningGoatsCanaryOverride`, an isolated test override; set only the **canary config** `override_item` to this Item. Do not change the real `FeederOverride` for tests.
- Optionally Number `LightningGoatsCanaryCount`, with a reviewed local test rule.

The canary rule validates/echoes UUIDs and records a command count; it contains **no physical Item writes, rule invocation, shell/HTTP/MQTT actuation, or links to the existing owner**. Inspect other rules/groups/links for unintended side effects. Record commands received separately from deduplicated results so the fixture cannot hide duplicate gateway dispatch. Implement deterministic held/late/failed acknowledgement cases in isolated fixtures, never by tampering with production results or corrupting live state.

Start locally on `127.0.0.1:8790`; later bind `10.8.0.6:8790` only after the approved network policy is in place. Keep `protocol="uuid_canary"`, separate persistent canary database, 5-second minimum interval, and 5-second acknowledgement timeout from the canary example. Use read-only real weather only after its local input is validated.

The shipped production/canary units share a Unix user and ciphertext path. Do not describe them as isolated credentials. Prefer a separate locked `lightning-goats-gateway-canary` user and a distinct canary token/ciphertext file via reviewed unit changes/drop-ins, retaining the in-service name `openhab-token`. OpenHAB USER tokens may still have equivalent Item rights; separate labels do not create a per-Item authorization boundary. The canary-only code/configuration and local access policy remain essential.

Only after reviewing the harmless rule and target binding, turn the **canary** remote-enable ON and exercise it. Verify the effective systemd configuration, permissions, credentials, actual listener and MAC labels. Do not disable SELinux/AppArmor or all firewalling to resolve a failure.

## 8. Preserve the physical-owner safety boundary

The current typed production adapter sends JSON `requestId` plus fresh UTC `requestedAt`, and recognizes the exact matching owner result. `accepted`/`running`, an HTTP success, an unrelated UUID, and an owner denial are not completed feeding. Bare UUID receipts belong only to the canary.

Read the owner-finality proposal before attempting physical enablement. The inspected owner can persist `complete` and later overwrite it with `failed` after downstream errors; admission also has pre-ownership state reads. A current Item snapshot or isolated JDBC `complete` row does not resolve that defect. `inspect-owner-history.py` is offline diagnostic evidence, not an authorization tool.

Finish source/fixture review and prepare the existing-owner correction if no other agent owns it. Requirements include exclusive admission before reading mutable admission state, a durable unresolved reservation before ON, no fresh actuation after uncertainty/restart, and terminal completion semantics that notification failure cannot reverse. Preserve other callers and the actuator OFF backstop. Define schema/version, restart, loss, compatibility and rollback behavior with the VPS-facing adapter. Do not introduce a second physical owner.

Code and harmless model tests may proceed now. Applying the correction to the home owner and performing a physical test need separate approval and verified backups. Until then, report `owner_finality: blocked`, leave production gateway inactive/remote-enable OFF, and still complete all independent canary, weather, credential and policy work.

## 9. Weather: local read only

Read only `/get_received_data` from the local weather receiver. Never call its `/weather` mutation endpoint in a health test or expose port 5000 to the VPS. Do not rebind, stop, replace or alter the receiver's station-ingestion path without understanding its existing clients.

Validate a sanitized real response against `src/gateway/weather.rs`: observation timestamp format/age, clock skew, explicit normalized units and malformed-field handling. Test old/future/regressing data with fixtures, not by injecting it into the actual receiver or changing the host clock. Test optional OpenHAB temperature with explicit Fahrenheit/Celsius units; omit the optional Item instead of inventing units when it is unavailable. Report unavailable weather honestly. The gateway serves structured data; the **VPS daemon** creates/schedules overlay-only weather messages. The home gateway should not run a second message scheduler or Nostr publisher.

## 10. Home-side UFW/WireGuard containment

Inventory before designing policy: actual tunnel/interface and routing path, peer `AllowedIPs`, UFW numbered/raw rules or the actual alternative manager, nftables/iptables IPv4/IPv6, input/forward chains, established-state rules, NAT, containers and listeners. Keep private/preshared keys out of output. Preserve local and independent administrative recovery.

Desired new-project access is only the approved staging VPS to canary TCP 8790; production TCP 8789 is a later gated allowance. No new VPS access to OpenHAB 8080/8443, weather 5000, SSH, PostgreSQL, or unrelated LAN/VPN services. Any administrative exception must be separately recorded and must not be counted as successful isolation.

**Adding a narrow UFW allow is insufficient if broad existing rules remain.** If the home peer accepts a full subnet from the VPS hub key, that hub can carry packets with any accepted source in that subnet. Filtering only source `10.8.0.1` is not a complete future compromised-hub policy. Preserve independently authenticated end-to-end administration rather than trusting a purported laptop address through the hub. A second tunnel on the same VPS does not remove a pre-existing broad trusted path.

Prepare two explicit change plans: (1) staging that preserves the currently authoritative legacy services, and (2) final home-enforced containment for the new hub after approved cutover. Do not break old LNbits/OpenHAB callers by silently applying final policy during parallel staging. A temporary staging exception is not final containment acceptance. If the existing topology cannot meet the target without affecting household access, present the bounded alternatives and rollback; keep the new gateway loopback-only meanwhile.

Use UFW on the home host if it is already the authoritative manager; do not install a competing manager over firewalld/nftables. No `ufw reset`, firewall flush, blanket subnet allow, global forwarding change, or blind IPv6 disablement. Provide plan/dry-run, drift checks, exact reversible application steps and console recovery. Apply household policy only after its specific approval. Never change the old hub or claim `10.8.0.1` here.

The current gateway has no application-level peer authentication. Its systemd subnet allowance and private-IP binding are not peer authorization. Do not invent a bearer-token requirement without coordinating client changes, and do not expose it to an untrusted interface while relying on a nonexistent authentication layer.

## 11. Verification and rollback

Record exact commands, source/config hashes, expected/observed results, timestamps and whether each result is mocked, local-live or cross-host. Minimum acceptance:

| Test | Required evidence |
| --- | --- |
| Identity/credentials | Runtime has no sudo/capabilities; binary/config not writable; only approved credential available; invalid synthetic credential fails closed |
| Safety | Missing/invalid/disabled canary safety state results in zero mock-owner commands; real safety Items unchanged |
| UUID replay/concurrency | Same UUID repeated, concurrent distinct UUIDs and two gateway processes sharing one test store never exceed one admitted unresolved command |
| Refusal/restart | Durable refusal/cooldown, pending restart, delayed acknowledgement and failed confirmation recovery preserve the original UUID; no resend |
| Happy path | Real daemon + real gateway + harmless owner: 2340 synthetic sats, two confirmed mock commands, 340 remaining, respecting the 5-second canary interval |
| Weather | Valid local input succeeds; malformed/stale/future data fails through fixtures; optional explicit-unit temperature behaves correctly |
| Network | Approved gateway reachable from actual staging path; direct trusted services and routed alternatives denied; authorized administration still works |
| Backup/restore | Stopped/quiesced gateway state preserved, paired with daemon evidence where applicable; restore never erases ambiguity or permits physical replay |

`GET /healthz` is liveness, not proof that credentials, weather or completion recovery work. Check `/v1/feeder/override`, `/v1/temperature` and `/v1/weather` separately. For canary requests use `POST /v1/feeder/request/<uuid>` and GET the same path; assert the matching JSON `request_id` and typed `status` (`confirmed`, `pending`, `ambiguous`, or `not_dispatched` with refusal). Do not treat a generic 2xx/204 as confirmation.

Run corruption/crash/database-failure cases only on disposable fixture stores. Coordinate the combined 2340-sat test with the VPS agent; it must seed synthetic credit in a separate canary database, never issue a real Strike payment. Never point that canary at TCP 8789.

Negative `nc`/curl failures alone are insufficient if nothing is listening. Correlate tested listeners, routes and firewall counters, including the approved forwarded/source-attribution cases. Do not run broad scans or spoof real clients on the household network; use an authorized isolated rehearsal for those cases, then bounded final-path checks.

Rollback stops/disables only the newly installed canary, preserves its database/evidence, removes only new approved rules/Items/units or restores their exact backups, and leaves OpenHAB, the weather receiver, the existing owner, old hub and unrelated household services unchanged. Do not use an old financial/physical-state backup as a routine rollback after newer actions. Pending state survives rollback and remains subject to owner reconciliation.

## 12. Deliver scripts and a short VPS handoff

Where installation work is not already scripted, add home-specific helpers under `deploy/scripts/` with offline tests under `deploy/tests/`. Prefer read-only inventory, configuration generation, verified inactive installation and bounded canary checks. Helpers should default to plan/dry-run; require explicit apply; refuse unexpected existing files/users, unsafe paths and active-owner targets; redact secrets; preserve state; and never combine installation with physical activation or DNS/VPN cutover. Label future helper names as new work until committed and tested.

Publish `docs/deployment/home-gateway-status.md` plus a sanitized source-pinned evidence file under `docs/testing/evidence/`. Send the VPS agent this compact contract:

```yaml
source_commit: <full tested SHA>
home_host: 10.8.0.6
stage: <local-canary-ready | network-canary-ready | blocked>
canary_url: <verified loopback or WireGuard origin>
production_url: http://10.8.0.6:8789/
production_service_active: false
production_remote_enabled: false
owner_protocol: feeder_request_v1
owner_script_sha256: <observed; not assumed>
owner_finality: blocked
canary_protocol: uuid_canary
request_item: GoatFeeder_ManualRequest
result_item: GoatFeeder_ManualResult
credential_name: openhab-token
credential_shared_with_vps: false
verified_runtime_role: <effective role or blocked>
weather: <verified | unavailable | blocked>
network_policy: <proposed | staging-verified | final-verified>
evidence: <repo paths and exact test results>
next_vps_action: <one concrete action>
operator_approvals_remaining: <specific changes/tests>
```

Populate from observations; this example is not a readiness assertion. Include the tested HTTP status/JSON contract, reachability scope, timeout behavior, configured caps, service/unit names and binary hash. Never include a token, private key, raw owner history or secret export. If schema changes are needed, give the VPS agent the reviewed commit and exact client change before switching protocols.

**Home-canary-ready is a useful completed milestone even while owner finality is blocked.** Full home production readiness additionally requires approved/verified physical-owner semantics, effective containment, safe state/restore, credential rights, final source checks and separately approved end-to-end physical acceptance. Keep unresolved findings explicit; do not describe code existence, an open port or a successful mock as production completion.

## Reference basis

Repository paths above were checked at the inspected commit. External references, consulted 2026-09-12, describe mechanisms; verify applicability to the installed versions:

- [OpenHAB REST authentication and roles](https://www.openhab.org/docs/configuration/restdocs) — USER rights and Implicit User Role.
- [OpenHAB API tokens](https://www.openhab.org/docs/configuration/apitokens.html) — supported token management and Basic-token convention.
- [systemd credentials](https://systemd.io/CREDENTIALS/) — named, service-scoped credential files and local encryption.
- [WireGuard cryptokey routing](https://www.wireguard.com/#cryptokey-routing) — peer keys and accepted packet-source prefixes.
