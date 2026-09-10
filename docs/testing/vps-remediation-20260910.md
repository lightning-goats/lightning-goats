# New VPS remediation evidence — 2026-09-10

Production remains **HOLD**. This record supplements earlier evidence; it does
not approve real payments, physical feeding, networking changes or cutover.

## Source and environment

- Source: `b2abf4af692773046ed9fee63f8810ecef4350f4`.
- PR #31 inspected through GitHub: open, draft, unmerged, unchanged at that SHA.
- Dedicated branch: `remediation/phase1-durable-admission-20260910`.
- No existing checkout or local repository changes were found. A fresh HTTPS
  clone was created under `/home/linuxuser/lightning-goats`; unrelated files and
  host maintenance notes were preserved.
- Hexmem: no callable interface, local executable, resource template or host
  instruction file found. Repository documentation supplies context.
- Fedora Server 44, x86_64, running kernel `7.2.4-200.fc44.x86_64`.
- Rust/Cargo 1.88.0 installed under the development account, with rustfmt/Clippy.
  Build uses one job and disables development/test debug information because
  this VPS has approximately 749 MiB RAM plus swap.
- nginx 1.30.4 installed for disposable loopback tests; no system nginx service
  was enabled or started by this work. OpenSSL 3.5.8, Python 3.14.
- The default execution sandbox denies network/socket use. Dependency retrieval
  and loopback tests use the approved development execution context. No home
  network access, production secrets or physical commands are involved.

## Baseline

Confirmed GitHub checks for the source SHA:

- [Rust CI — passed](https://github.com/lightning-goats/lightning-goats/actions/runs/34523490824)
- [Security — passed](https://github.com/lightning-goats/lightning-goats/actions/runs/34523490779)
- [Deployment artifacts — passed](https://github.com/lightning-goats/lightning-goats/actions/runs/34523491033)

Local format check passed before edits. Initial deployment tests could not bind
loopback sockets in the default sandbox; rerun in the approved test context is
recorded below. The initial Rust run likewise failed seven socket-dependent
tests in that sandbox; the rerun passed all 80 existing unit tests and the canary
test before reaching the deliberately failing new F02 regressions. Existing CI
is not evidence for a subsequent remediation SHA.

## F02 regression evidence

The handoff implementation failed both new command-count regressions:

- Two distinct UUIDs sent concurrently to two real gateway processes sharing
  one SQLite file caused **two** mock OpenHAB commands (expected one).
- A database failure while persisting completion left a pending row, but a new
  UUID still caused another command.

After the atomic admission correction, both tests pass. They also exercise
same-ID replay, timeout followed by a new UUID, process restart with pending
state, late acknowledgement, and interval refusal. Only the mock's synthetic
UUID echo is used; it is not a fixture from the physical owner.

Additional tests cover the rolling hour cap after process restart, expiration
of one synthetic history entry, durable reservation/completion audit records,
legacy multiple-pending rows and SQLite memory URI aliases. The independent
read-only review found no concrete surviving shared-store F02 bypass.

The nginx harness initially failed because Fedora's default fastcgi temporary
directory was root-owned. Commit `944841cfd096c7390e743bb5d3546df740fd9f64`
isolates all temporary paths and startup logging. All **11** deployment tests
then passed as the development user with temporary loopback services.

Raw development logs are retained outside the repository under
`/home/linuxuser/lg-evidence/`; only sanitized summaries belong in this record.

F02 source commit: `405dd852b5f93bfa8b4a311681b1bd4acd4e4db8`,
[draft PR #32](https://github.com/lightning-goats/lightning-goats/pull/32).
Local format, locked Clippy and full locked tests passed: 82 unit tests plus
eight integration tests, including three real-gateway process tests. GitHub
[Rust CI passed](https://github.com/lightning-goats/lightning-goats/actions/runs/34526370025)
and [Security passed](https://github.com/lightning-goats/lightning-goats/actions/runs/34526369958).
The [deployment run](https://github.com/lightning-goats/lightning-goats/actions/runs/34526369903)
passed the real three-binary release build/archive/checksum/help smoke job, but
its nginx job failed on Ubuntu 22.04's nginx 1.18: the new `-e` startup option is
unsupported there. The follow-up removes that option while retaining all
temporary-path overrides; all 11 tests pass locally again. Require green checks
on the follow-up head before accepting the artifact gate.

## External dependencies and acceptance

No authorized outbound SSH identity, home-owner fixture, authoritative website
source, assigned WireGuard peer inventory or staging hostname was present in
the inspected workspace. Read-only access instructions were requested. F04's
physical binding and F09's home policy remain blocked pending that evidence.
No staging address was chosen. The known addresses remain old hub `10.8.0.1`,
trusted home host `10.8.0.6`, and operator laptop `10.8.0.10`.

The inspected host maintenance notes permit password SSH and root public-key
SSH; production account/hardening acceptance remains open. No SSH policy was
changed during remediation.

The parent gates #6/#15/#16 remain open. Clean-install ownership/credentials,
home containment, authoritative website/browser, signer/relay and physical
acceptance are distinct from the isolated tests below.

The operator subsequently supplied a staging WireGuard template for `.12` and
`fd5e:06df:9c82::12`, hub endpoint `45.76.234.192:51820`. Local inventory found
only loopback and the public interface, with no existing WireGuard listener.
A new restricted local key/config was prepared and syntax-checked, outside Git.
Peer-address reservation and hub registration remain pending; no tunnel, route,
firewall rule, existing peer identity or production endpoint was changed.

## F03 isolated recovery evidence

The follow-up branch `remediation/phase1-correlated-recovery-20260910` starts at
the F02 review head `6ac9363f8951ad232fc9a4e48380de90e2f06748`. All three GitHub
workflows passed for that F02 head:
[Rust](https://github.com/lightning-goats/lightning-goats/actions/runs/34526938256),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/34526938289),
[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/34526938376).

Six process integration cases pass locally for the F03 candidate. In addition
to the F02 cases, the actual daemon binary and actual gateway binary, configured
from shipped canary examples with loopback paths and synthetic credentials,
produced exactly two mock commands and two `feeder_confirmed` events from
2340 synthetic sats, retaining 340 sats across daemon restart. The shipped
five-second inter-feed/minimum timing is used. No real payment was initiated.

Additional process cases lose the response after the mock command, restart the
daemon, inject a failed confirmation transaction, then reconcile a late result
with the original UUID and exactly one command/debit. A lost 429 refusal is
recovered through GET, its cooldown survives daemon restart, and replay of the
refused UUID remains terminal after capacity becomes available. Synthetic
database timestamps are advanced for hour/cooldown tests; the host clock is not
changed. Unit cases exercise contradictory/wrong-UUID/legacy responses and a
same-ID admission race with conflicting safety snapshots.

The independent candidate review found no concrete F03 bypass. F04 remains open:
these results establish the internal protocol with a harmless echo fixture,
not receipt-versus-completion behavior of the actual household owner.


## F05 durable settlement recovery candidate

Source base: `8580babd03733a9679d4d5f2183ab9c673806241`, draft
[PR #33](https://github.com/lightning-goats/lightning-goats/pull/33). That head passed
[Rust CI](https://github.com/lightning-goats/lightning-goats/actions/runs/34527761238),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/34527761248), and
[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/34527761348).
The settlement follow-up uses branch
`remediation/phase1-settlement-recovery-20260910` and the same Fedora/Rust environment.

Authenticated webhook IDs are persisted before HTTP 204. Conflicting event IDs
fail closed. Provider reads run outside the HTTP request, with persisted retry
backoff and an independently scheduled, paginated scan of every locally issued
request. Full scans repeat because provider offset pagination is not a stable
snapshot. Scan progress and discovered work survive restart. Inbox admission is
capped at 10,000 pending records; exhaustion or database failure returns 503 so
notifications remain retryable. One worker iteration processes at most one inbox
entry and one 100-row provider page. Provider bodies are bounded while streaming
at 256 KiB; redirects are disabled. These are application bounds, not a complete
F08 acceptance claim.

Focused mock tests cover notification-free pagination, restart between discovery
and processing, exact duplicates, conflicting identities, 429/503 outages, and
recovery after the credit transaction commits but before inbox completion. They
assert one credit/event. The HTTP test exercises the actual application router:
slow provider causes zero calls during acknowledgement, duplicate delivery has
one row, reopen retains work, injected insertion failure returns 503, malformed
signature/content type/method fail, and a chunked oversized body receives 413.

F14 persistence and settlement now use the configuration/LNURL username
validator, including dotted usernames. Direct invalid issuance fails before
provider contact. This candidate still uses synthetic provider wrappers: F06
credited-currency policy and F10 cryptographic BOLT11 validation remain open.
No real invoice, payment, owner operation or network change is part of this evidence.


Candidate review identified and regression-tested two recovery liveness gaps:
new work now uses its creation time as its first due time, so overdue page/retry
work cannot be starved by a continuous stream of new rows. After eight failed
attempts, work moves to durable quarantine, outside pending admission capacity,
and remains eligible for hourly authoritative retry. It is never credited or
discarded on error. A full 10,000-row admission test proves that quarantining a
poison item releases a slot while retaining retry/completion capability; the
outage test recovers quarantined work after provider restoration. Audit records
are retained; database growth/retention and F08 global ingress budgets remain
separate operational resource controls.


Final local candidate checks: `cargo fmt --all --check`,
`cargo clippy --locked --all-targets --all-features -- -D warnings`, and
`cargo test --locked --all-features` pass (89 library tests, one daemon HTTP test,
11 integration tests). All 11 deployment tests pass. The candidate review's two
concrete liveness issues were corrected and their regressions pass. Exact-head
GitHub Rust, Security and real-binary packaging results must be attached after
publication; local Rust/HTTP tests are not a substitute for dependency audit.


## F06/F10 signed-invoice and target-currency candidate

Base: F05/F14 commit `cc23a4f2dfce7eaa8492f3ddf40a129e22bef309`,
[draft PR #34](https://github.com/lightning-goats/lightning-goats/pull/34).
That base passed [Rust CI](https://github.com/lightning-goats/lightning-goats/actions/runs/34529555784),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/34529556111), and
[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/34529555790).
The next source branch is `remediation/phase1-verified-invoices-20260910`.

Real BOLT11 fixtures are signed locally with an explicitly synthetic test key;
no invoice is obtained from a real provider or paid. Tests distinguish valid
checksum from invalid signature, reject wrong network, amountless/mismatched
amounts, wrong hashes, bad checksum, expiry mismatch, expired issuance and
future timestamps. Delayed settlement retains signed verification without an
issuance-time expiry rejection. A provider wrapper with a plausible fake invoice
fails; absent optional wrapper fields still require a verified signed contract.
The LNURL callback regression asserts a fake invoice produces an error and zero
issued rows, while a dotted configured user survives discovery/callback/storage.

P2P fixtures include USD received with a different authoritative BTC credited
amount. They assert that only credited BTC becomes feed credit, replay creates
one event, the original amounts remain in context and no payment hash is invented.
Missing/non-BTC/zero/fractional-satoshi/conflicting BTC-to-BTC credited amounts
produce no credit or event. Policy is exact positive whole sats, with no rounding.
The independent receipt/retry mechanisms retain unsupported cases for review.

The actual provider documentation was read, but no live P2P account observation
or account fixture was authorized/available. Mainnet is explicitly required;
canary provider network compatibility is still a live acceptance dependency.


The read-only candidate review found no concrete F06/F10 bypass. Its confirmed
F14 edge case is corrected: whole usernames `.` and `..` are rejected centrally
because URL joining would collapse their identity. Configuration tests reject
both while retaining `goat.name`; the real LNURL service callback/persistence
test covers the ordinary dotted case. Existing financial conflict checks and
receive-only credentials remain unchanged.


Local validation at publication: format, locked Clippy and all 11 deployment
tests pass. The full locked suite passed before the added explicit expired-
invoice recovery regression (94 library tests, one daemon HTTP test, 14
integration tests). The final locked rerun including that additional case also passed: 95 library
tests, one daemon HTTP test and 14 integration tests. Attach exact-head GitHub
checks before accepting the candidate. Logs are under `/home/linuxuser/lg-evidence/f06-f10-*`.


## F08 application resource candidate

Base: `7f54df087a0eaba1df6873e5318cb4b6143f5b8c`,
[draft PR #35](https://github.com/lightning-goats/lightning-goats/pull/35), with passing
[Rust CI](https://github.com/lightning-goats/lightning-goats/actions/runs/34531310426),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/34531310469), and
[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/34531310293).
The next branch is `remediation/phase1-resource-controls-20260910`.

Invoice creation validates input before nonwaiting admission. A process semaphore
bounds tasks before SQLite; a shared-store transaction enforces four concurrent
reservations and 30 provider attempts per rolling minute across all users and
processes. Failed provider attempts still consume rate budget. Reservations
expire after 30 seconds on crash/cancellation; an admitted provider/persistence
operation has a 20-second deadline. Normal completion releases capacity. Recovery
and webhook work do not acquire these issuance permits.

The candidate also stops new issuance when issued requests from the last 24 hours plus active
reservations reach 10,000. This rolling budget restores capacity as the window advances;
no financial history is deleted. It is not a database retention policy. Historical
storage/archival and operator sizing remain operational acceptance decisions.
Recovery continues independently while new issuance is refused.

HTTP handlers have 64 nonwaiting slots and a 30-second whole-handler/upload
deadline; public status reads have four slots. WebSocket upgrades have 32 permits
held through socket lifetime, 1-KiB inbound frame/message bounds and 64-KiB write
buffer bounds. F11 still must establish heartbeat, idle expiry and replay semantics.

OpenHAB item state is streaming-bounded at 4 KiB with strict UTF-8; weather uses
the shared 64-KiB streaming bound and supports chunked responses. Both reject
redirects and non-success statuses. Mock tests assert redirected reads and
commands never reach a second server, while gateway health rejects 3xx rather
than mistaking it for success. These transport checks do not bind the actual
physical owner's receipt/completion contract (F04 remains open).

Focused library tests pass for global/restart admission, concurrency and capacity,
exact chunked bounds, oversized/invalid UTF-8 states and 307/308 redirect rejection.
HTTP and LNURL tests cover excess requests across users, zero provider calls for
invalid/budget-exhausted requests, overload without a waiter queue, upload timeout
and webhook persistence while invoice budget is exhausted. Attach final full-suite
and GitHub results after completion; F11/F12 and deployment acceptance remain open.


Candidate review confirmed three gaps and drove corrections: LNURL overload/
timeout errors retain their JSON envelope; the daily issuance window ages out
without deleting issued history; and both binaries now use bounded HTTP/1.1
transport upstream of handler middleware. Transport allows 128 active ordinary
HTTP connections, 32-KiB header buffers, a real 10-second header timer and a
180-second connection lifetime (including response transmission), longer than
the gateway's 140-second handler/150-second client deadlines. WebSocket upgrades
transfer to their separately bounded socket lifetime. nginx remains the public
HTTP/TLS edge; its upstream uses HTTP/1.1. Gateway shutdown drains existing
connections within the bounded lifetime. Incomplete-header tests assert excess
connections are refused, stalled headers expire and capacity becomes reusable.


At draft publication, 101 library tests and locked Clippy pass, including
incomplete-header admission/expiry. All 11 deployment tests pass. The final locked
full suite includes actual HTTP upload timeout and overload envelopes, daily
budget age-out without history deletion, interrupted lease expiry and existing
settlement recovery while public issuance is saturated. Its result and exact-head
GitHub checks remain required before acceptance. No live system tests occurred.

## F08 final evidence and F11/F12 continuation

F08 source `672763db6d36c696424e24fab92989a7c1b95f8a` in
[draft #36](https://github.com/lightning-goats/lightning-goats/pull/36) passed the
full local locked suite: 101 library tests, two daemon HTTP tests, 15 integration
tests, format and locked Clippy. All 11 deployment regressions passed.
Exact-source [Rust CI](https://github.com/lightning-goats/lightning-goats/actions/runs/34533305207),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/34533305205), and
[Deployment artifacts](https://github.com/lightning-goats/lightning-goats/actions/runs/34533305210)
passed. This completes the earlier pending local-run record, not production acceptance.

F11/F12 continues from that exact source on
`remediation/phase1-overlay-weather-20260910`, same Fedora VPS/Rust 1.88 and
isolated loopback-only environment. The first library run passed 109 tests,
including actual WebSocket replay/reset/control-frame behavior and 76-second
quiet heartbeat survival. The final candidate additionally checks stream identity
persistence, socket saturation/reuse through the daemon's real router, unread
output deadlines, weather expiry while queued and restore sequence reuse.

The overlay has versioned last-sequence resume, ordered bounded replay before a
consistent checkpoint, explicit reset markers, per-send deadlines, receive-only
input, Ping/Pong liveness and bounded socket lifetime. See
`../architecture/overlay-stream.md` for client deduplication/reset rules.

Weather uses parsed observation age, bounded future skew and a durable monotonic
watermark, including on first read and restart. Invalid observations do not
advance the watermark. Normalized keys declare units; explicit Celsius is
converted to Fahrenheit and unitless OpenHAB display states are rejected.
Stale weather replay preserves the cursor with a skip marker. These presentation
changes do not create ledger credit, feed attempts or Nostr outbox entries.

Independent candidate review found two concrete transitions: queued weather may
expire before sending, and restoring older history may reuse acknowledged
sequences. Freshness is now checked again when dequeued. Offline restore must run
`lightning-goatsctl --config <restored-config> reset-overlay-stream` before any
daemon starts, rotating only presentation identity. Ordinary process restarts
retain identity for resume. Raw database rollback without that step is unsupported.
Regression tests cover both transitions; final locked suite/exact-head CI results
must be attached after completion. Logs: `/home/linuxuser/lg-evidence/f11-f12-*`.

Actual weather/owner fixtures, authoritative website/browser behavior, network
containment, clean install, paired-store restore and signer acceptance remain
separate gates. No tunnel was activated, no production settings changed, no real
provider invoice/payment created and no physical owner contacted. Production HOLD
and parent #6/#15/#16 remain open. Keep the stack draft until integrated review.

## F11/F12 final evidence and paired-store restore candidate

Source `a3b17e809ba956407a98c35a07685f3d61402050`,
[draft #37](https://github.com/lightning-goats/lightning-goats/pull/37), passed
format, locked Clippy and the full locked local suite: 113 library tests, two
daemon HTTP tests and 15 integration tests. This includes default 76-second
quiet heartbeat survival, slow-reader send expiry, 32 actual-route socket permits
with reuse, receive-only frame limits, queued-weather expiry, restore sequence
reuse, and all prior daemon/gateway command-count regressions.
[Rust CI](https://github.com/lightning-goats/lightning-goats/actions/runs/34535661549),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/34535661691), and
[Deployment artifacts](https://github.com/lightning-goats/lightning-goats/actions/runs/34535661601)
passed for that candidate. All 11 local deployment tests passed.

The next branch, `remediation/phase1-staging-evidence-20260910`, starts at that
exact source and adds an offline paired-store restore helper and real-process
recovery regression. Three Python restore failure/permission/hash tests pass,
and locked Clippy passes. Full locked Rust and the expanded 14-test deployment
suite remain required at publication; final results will be appended after they
complete. Logs: `/home/linuxuser/lg-evidence/acceptance-*`.

See `../deployment/staging-acceptance-evidence.md` for the reviewed procedure,
mock scope, F09 inventory/test matrix and unresolved external acceptance gates.
The Deployment workflow now retains its exact tested package for 14 days under
`staging-package-<GITHUB_SHA>`. On pull requests that SHA is the tested synthetic
merge commit: read BUILD-INFO and retain it alongside the PR head, never relabel
it as the branch head. Earlier PR workflow runs tested but did not retain a
staging artifact. Artifact retention follows the supported
[GitHub upload-artifact action](https://github.com/actions/upload-artifact).

No actual home inventory, complete household policy review, final installed
service/credential acceptance, authoritative website import/browser check or
signer/relay acceptance is claimed. Those dependencies and parent gates stay open.


The initial staging-evidence Deployment run
[34536522612](https://github.com/lightning-goats/lightning-goats/actions/runs/34536522612)
failed only while Python restore tests constructed fixtures: Ubuntu 22.04's
SQLite 3.37 lacks `unixepoch()`, while the actual Rust daemon bundles a newer
SQLite. The fixture now supplies a fixed test-only clock on SQLite < 3.38;
production migration SQL and restore validation are unchanged. The package build,
archive verification and retention job passed. A connector-provided download
reference returned HTTP 403 from this VPS, so local installation of that archive
was not observed. Require final corrected-head CI before accepting the candidate.
The local full locked Rust suite completed successfully: 113 library tests, two
daemon HTTP tests and 16 integration tests. The new paired-store restore test
passed with exactly one total mock-owner command across backup/restore/restart,
one late confirmation/debit and unchanged issued request and signed outbox bytes.
The subsequent change is limited to Python fixture compatibility and acceptance
documentation; its focused restore tests pass. Format and locked Clippy passed;
all 14 local deployment tests passed before the fixture portability correction.
Final GitHub results are recorded on draft PR #38; live acceptance remains open.

## Installed-release rehearsal candidate

Base: PR #38 head `76407c0ba8821ee53b4ffcb044e5a6d9b6733537`, clean and still
draft when rechecked. The continuation branch is
`remediation/phase1-install-rehearsal-20260910`. Final base CI/Security/Deployment
passed; no merge or production changes occurred.

This candidate reuses archive verification without executing payloads as root,
then rehearses an installation inside a new loopback-only network namespace.
It checks root-owned binaries/config, two distinct unprivileged identities with
no effective capabilities and NoNewPrivileges, credential separation, state
ownership/migrations, six discovery routes, gateway/temperature reads and exact-ID
restart idempotency using only its internal harmless owner. No accounts or host
services are created, and the launcher refuses the host namespace.

All 17 local deployment tests pass, including the three new namespace/fixture-drift
safety regressions and the existing six archive regressions. Actual creation of
an isolated namespace succeeded. A local locked release build of the exact base
is running to exercise the new harness against real release binaries; final local
and CI installation evidence must be attached after completion. CI runs the same
harness and retains its JSON with the outer package checksum manifest.

See `../deployment/install-rehearsal.md`. Synthetic file credential/process
rehearsal does not close final systemd encrypted-credential, public TLS/IPv6,
actual owner, browser, signer, home containment or deploy-privilege gates. The
operator was asked for authorized read-only owner/site access and confirmation
of .12 reservation/peer registration; these are pending. Production remains HOLD.

Installed-release execution completed on both Ubuntu CI and the Fedora VPS.
Initial harness source `09cbe604aa451d48ae0d06fdf5b7a6b20ac7ef85` passed
[Rust CI](https://github.com/lightning-goats/lightning-goats/actions/runs/34538395098),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/34538394993), and
[Deployment including installed rehearsal](https://github.com/lightning-goats/lightning-goats/actions/runs/34538394981).
Its retained package contains `INSTALL-REHEARSAL.json` and outer checksums.

On Fedora, `cargo build --release --locked --bins` completed for exact runtime
source `76407c0ba8821ee53b4ffcb044e5a6d9b6733537`. Full archive checksums/source
verification and all three help paths passed, followed by actual installed
process/credential/state/discovery/restart checks with one total mock-owner
command. The temporary installation and processes were removed. A generated
Python bytecode cache was identified and removed specifically; the harness now
disables bytecode writes, and the repeated Fedora run passed with no cache left.
See the sanitized record `evidence/installed-release-fedora-20260910.json` for
runtime/harness/archive/binary hashes and actual UID/GID/capability evidence.
Final PR #39 checks are still required for the subsequent cache/evidence commit.

This closes the isolated installed-release rehearsal gap. Final systemd sandbox,
encrypted credential delivery, production identity/privilege revocation, TLS/site/
browser, trusted network, actual-owner and live operational gates remain open.
