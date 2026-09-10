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
