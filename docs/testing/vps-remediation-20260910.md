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
