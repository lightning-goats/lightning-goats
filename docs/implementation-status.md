# Lightning Goats implementation status

Current CyberHerd checkpoint: **2026-11-11**. The full
[twelve-part implementation plan](planning/cyberherd-implementation-plan.md)
remains incomplete. It supersedes older CyberHerd deferrals; it does not resume
Monero or authorize financial/physical activation.

The operator selected **3 user spots** and reuse of the existing messaging
identity for operator authentication. The [configuration and signed-settings
adapter](deployment/cyberherd-project-operator.md) preserves the current reward
percentage and uses the household bunker. The [live signed command and public
capacity readback](testing/cyberherd-three-user-capacity-live-20261111.md) now pass:
capacity 3, zero active members and unchanged 10% allocation. Only membership was
restarted; core payments/feeding and outgoing roles retained their original state.

The existing receive service is running after the operator-approved
[live core upgrade](testing/cyberherd-live-core-upgrade-20261110.md), preserving
paid accounting, IDs, epoch, exact signed bytes and configuration. The
[three separate CyberHerd roles](testing/cyberherd-live-inactive-installation-20261110.md)
are installed under locked non-admin users. The approved
[live overlay stage](testing/cyberherd-live-overlay-stage-20261110.md) now runs
membership with native incoming zaps and a zero-budget core bridge. Payout/signer
remain masked/inactive; rewards, membership Nostr and lifecycle stay disabled.

[Installed OS probes](testing/cyberherd-installed-qualification-checkpoint-20261110.md)
and [actual native worker checks](testing/cyberherd-worker-qualification-checkpoint-20261110.md)
passed with synthetic credentials/private journals. Four separate native users
completed socket boundary checks, restart and native four-journal snapshot
verification while preserving original production journals and receive process.
These fixtures do not establish live transfers or external recovery adoption.

The [native recipient/DNS checkpoint](testing/cyberherd-recipient-checkpoint-20261110.md)
now passes exact CI and actual-host checks: credential-free public LNURL metadata
validation plus exact private IPv4/IPv6 and mixed-answer refusal before HTTP.
Only temporary units/fixture mounts were used. The subsequent
[actual host network checks](testing/cyberherd-host-network-checkpoint-20261111.md)
verify the running member's effective kernel filters, three credential-free
CyberHerd identity probes, the receive UID's gateway/private-path policy and
certificate-verified public IPv4/IPv6 TLS. Original application files/PIDs/masks
and the full firewall ruleset stay exact; no provider API or feeder request runs.
Functional forwarding from another peer, public inbound IPv6, all recipients and
external recovery adoption remain separate.

The separate [payout credential](deployment/cyberherd-payout-credential.md) is
host-encrypted and passed receive-request/same-core-resource read checks after
the operator corrected an omitted scope. Payment-read, quote/execute and controlled
payment qualification remain unfinished; no payout was sent.

The [four-role financial candidate preparer](deployment/cyberherd-financial-stage.md)
binds explicit operator limits, an already selected stored allocation and all four
native configurations. It preserves running services and creates no activation
authority. Its [qualified checkpoint](testing/cyberherd-financial-candidate-checkpoint-20261111.md)
passes all ten workflows, 522 ordinary native tests, separate root checks and the
installed nonzero-candidate rehearsal. The selected net limit now applies to both
ordinary Lightning and zaps. A new authenticated 24-file tool generation and
checker are installed separately on the VPS. The actual zero-only review safely
refused before directory creation because the stored member allocation is 10%;
all 17 original files/PIDs and zero spending limits remain exact. Requested
operator limits remain unanswered, and no actual-host financial candidate or
activation has been performed.

The new native `review-rewards` command reviews only accepted four-journal backup
images, preserving the original manifest format. It reports frozen feeding
allocations, unplanned/closed/reserved work, pending intake and cursor lag without
creating plans or granting activation authority. Its
[qualified checkpoint](testing/cyberherd-reward-backlog-checkpoint-20261111.md)
passes all ten workflows, 526 ordinary native cases, separate protected-root
checks and the actual installed CLI rehearsal. A separate VPS reader also
accepts a historical backup without changing its images or live applications;
the private findings do not establish current activation readiness. See
[the backlog review contract](deployment/cyberherd-state-maintenance.md#review-retained-reward-backlog).

A separate [six-program maintenance transaction](deployment/cyberherd-program-maintenance.md)
now prepares original/selected programs, verifies a fresh native four-journal
backup with both verifiers, reviews its backlog and upgrades only executable bytes.
It retains zero financial limits and masked outgoing roles, with explicit separate
resumption and pre-resumption program rollback. Its
[qualified checkpoint](testing/cyberherd-program-maintenance-checkpoint-20261111.md)
passes all ten workflows, 526 ordinary native tests, 131 protected-root Python
cases and the installed native/systemd transaction. Actual VPS preparation and
independent staged/original file/process readback pass without opening journals
or changing running applications. No actual-host maintenance, financial candidate
application or outgoing activation has been performed.

The [first financial configuration transaction](deployment/cyberherd-financial-maintenance.md)
is now implemented separately in source. It consumes the sealed candidate and
completed selected-program review, takes a fresh stopped-writer backup/review,
refuses unreceived financial events or historical funded work, and separates
configuration application from explicit four-role activation. A native review
distinguishes payment/feeding cursor tails from ordinary overlay lag. Source,
protected-file and disposable installed qualification are being completed; no
actual-host financial transaction or provider execution is established.

Separate NIP-46 clients now pass [actual household signing checks](testing/cyberherd-bunker-checkpoint-20261110.md):
membership kinds 1/1311 and payout-signing kind 9734 preserve the exact project
author, frozen unsigned bytes, canonical hashes and valid signatures. The operator
found and stopped a competing bunker on the streaming laptop after inspections
confirmed the intended household instance and local client selection. The final
connect observation saw one acknowledgment and no authorization refusal/conflict.
Pinned VPS/household nak versions also pass isolated four-client interoperability
and bounded observation tests. No key was regenerated, project event published,
invoice requested or payment sent. The separate
[native kind-9735 receipt check](testing/cyberherd-zap-receipt-checkpoint-20261110.md)
also passes on the VPS using a synthetic invoice, with no ledger/provider/publication
operation and no installed application binary replacement. Live incoming-zap
settlement and actual CyberHerd/receipt public delivery remain separate;
only membership/overlay intake is now active, with outgoing roles masked/zero.

The [overlay-stage preparation checkpoint](testing/cyberherd-overlay-stage-checkpoint-20261110.md)
now passes exact CI and actual-VPS configuration-only preparation. A private
bundle preserves all six payees and the original core epoch/cutoff, keeps payout
limits zero and omits membership Nostr signing. Independent readback confirmed
unchanged installed configuration, inactive journals and running receive process.
That checkpoint prepared configuration without activating the new feature.

The [maintenance executor checkpoint](testing/cyberherd-overlay-maintenance-checkpoint-20261110.md)
now passes a complete installed systemd/native backup/apply/resume/rollback
rehearsal with synthetic state and isolated networking. The actual VPS execution
plan passed independent readback. The operator-approved apply verified a current
native backup; resume refused an inherited Fedora timeout policy before starting
applications. Configuration rollback and original-core resume succeeded. The
bounded correction then passed all ten workflows, root recovery tests and the
actual installed rehearsal. A fresh approved transaction verified a new native
backup and successfully resumed core plus membership. The actual public OBS
WebSocket receives the roster with no browser errors; outgoing roles stay masked.
See the [live checkpoint](testing/cyberherd-live-overlay-stage-20261110.md).

The [live social-intake correction](testing/cyberherd-live-social-intake-20261110.md)
now verifies the operator and an earlier user’s reaction/repost, exact awards,
three retained overlay messages and both actual OBS cards. The membership-only
binary update preserved the running core, configurations, schemas and paid state.

The [public readonly roster](testing/cyberherd-live-readonly-site-20261110.md)
now works at `/leaderboard` with exact API, disabled auth/operator routes, actual
browser rendering and WAF checks. The nginx-only release and exact SELinux port
label preserved application PIDs, payment settings and OBS/artwork.

The operator-requested [website payment/zap display](testing/website-payment-chat-checkpoint-20261110.md) is now deployed after all ten workflows and actual public browser/HTTPS acceptance. The leaderboard header, invisible responsive QR target and one-action WebLN flow preserve the running core and OBS. Live paid-zap acceptance remains separate. The [private contact source](testing/website-contact-checkpoint-20261110.md) implements free identity-bound NIP-17 delivery with ciphertext-only retry; its form is now included in the subsequent live contact release.

The separate [private contact fee implementation](testing/website-contact-fee-checkpoint-20261111.md) has default-off native purpose/settlement/outbox and exact ingress source. Combined browser/backend, enabled HTTP admission, native private-state correlation and strict additive four-journal comparison pass all ten workflows and 516 regular native tests plus the required separate root fixture. The [maintenance executor](deployment/private-contact-upgrade.md) passes root and actual installed migration rehearsal, then [actual VPS upgrade/resumption](testing/private-contact-schema-live-20261111.md) with a current backup and original-UID clone/live comparisons. The subsequent [actual contact deployment](testing/private-contact-live-20261111.md) enables only contact configuration, exact private ingress and the browser form after a fresh native four-journal backup. The 28-sat paused-feeder state, all existing programs/flags and OBS/artwork are preserved. Corrected serving-label handling passes exact CI and actual desktop/mobile public acceptance; real paid-contact and recipient-side DM observation remain open.

The current overlay stage now has verified [boot wiring](testing/cyberherd-boot-persistence-20261111.md) with the existing reviewed membership unit enabled and unchanged live PIDs/configuration at that step. No host reboot or new feature activation was performed. A separate [installed operator HTTP exercise](testing/cyberherd-installed-operator-checkpoint-20261111.md) passes native signed requests, real member restart and durable nonce/command retry checks on synthetic isolated CI state. The full native suite exposed a [feed-intent contention failure](testing/feed-intent-contention-20261111.md); its immediate-write correction and native WAL regression pass all ten workflows. The subsequent [current-schema update](testing/current-schema-core-patch-20261111.md) passes source `640331c178bfe1214a2911e708a21ad5c607b006`, actual native/systemd rehearsal and protected VPS apply/resume. The corrected core is running with exact live database images preserved during replacement, unchanged contact/configuration, 28-sat credit and paused feeding. Core/member remain enabled; payout/signer remain masked and all CyberHerd monetary/publication gates remain off.

The subsequent installed activation/score/ban/reset extension passes exact native
CI and installed systemd checks on source `886e6438b83497f0ef4db9931620a21941c676a9`.
Its bounded 100-ms bridge read-turn correction is now
[deployed](testing/cyberherd-bridge-turn-live-20261111.md) with a fresh verified
native backup, exact unchanged paid images and resumed core/member services.
The fixture verifies the committed normalized public projection, paused controls,
manual reset and original receipts across actual restarts with zero gateway POSTs.
Optional authenticated TLS/nginx transport and eleven isolated actual-stack WAF
cases also pass. Subsequent source `124809fc0b410f4e9d25cc26d24f5b11998bead7`
passes all ten workflows and a combined verified-TLS/nginx/native operator
rehearsal, including exact signed query/body transport, refusals and command retry.
The subsequent empty-operator [self-read transaction](testing/cyberherd-self-read-backend-live-20261111.md)
passes source `548276f0f68674073ec583ee7fcffc0237a4c2dc`, all ten workflows,
eight new protected-root cases and actual installed native qualification. Its
actual-host backup/apply/resume is complete: only member auth configuration changes,
paid images/programs/core configuration stay exact, and signed loopback self reads
plus nonce/operator refusals pass. Actual operator ingress remains disabled pending the selected
public-key allowlist. See the retained
failures, final artifact acceptance and preparation evidence in the
[operator checkpoint](testing/cyberherd-installed-operator-checkpoint-20261111.md).
The subsequent [public self-status release](testing/cyberherd-self-read-public-live-20261111.md)
passes all ten workflows on `e6110f1b0b902b0bdbea54987835f70388092160`, including
41 browser cases, 110 protected-root cases and actual signed TLS/nginx/native
self-read with closed operator ingress. Three assembled nginx and thirteen WAF
cases also pass. Its actual nginx-only deployment preserves all 17 application
files/PIDs and the 30-file public release's metadata/artwork, changing only the
auth client and auth-enabled flag. Actual public desktop/mobile synthetic-signer
self reads, nonce replay refusal, operator 404s and overlay WebSocket pass.
The empty operator allowlist, zero financial limits and masked outgoing roles
remain; no application restart, payment, command or feeding occurred.

Other remaining work includes observed zap admission and incoming settlement/
receipt delivery, operator interfaces, global network qualification, concrete externally reconciled recovery adoption, operator-selected
nonzero financial ceilings and explicit activation. Native protected
[four-journal maintenance](deployment/cyberherd-state-maintenance.md) preserves
replay/signed history and blocks staged restore startup pending reconciliation.
No live financial restore or automatic marker clearance is supported or implied.
Keep PR111 draft and the full goal active until every planned behavior and actual
qualification is complete.

The Phase 1 implementation history below was last updated on 2026-09-07; consult
the dated restoration/implementation records and current plan for later work.

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
# Audit correction: production HOLD

As of 2026-09-10, the 2026-09-08 audit supersedes the completion/readiness claims
retained below as historical evidence. Several blockers require Rust code and
integration tests, not deployment configuration alone. See
[`deployment/audit-remediation.md`](deployment/audit-remediation.md) for finding
status and [`deployment/deployment-artifacts.md`](deployment/deployment-artifacts.md)
for the corrected nginx/release workflow. Keep tracker #6 open until all required
source, integration and live acceptance evidence is recorded.
