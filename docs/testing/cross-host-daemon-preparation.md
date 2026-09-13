# Inactive cross-host daemon preparation

Production HOLD. This prepares a fresh synthetic ledger and canary configuration;
it starts no daemon, opens no listener and contacts no gateway or provider.
It does not establish cross-host, payment-provider or physical acceptance.

From a source-pinned checkout, choose a new absolute directory under an existing
canonical parent and run:

```sh
cargo run --locked --all-features --example prepare_cross_host -- \
  /absolute/new/session-directory "$(git rev-parse HEAD)"
```

The helper creates a private directory, mode-0600 synthetic credentials, and a
configuration derived from the shipped canary example. It retains canary mode,
the 1,000-sat threshold, five-second inter-feed delay and the shipped canary
gateway target. The provider is changed to an unavailable loopback HTTPS endpoint.
No real credential is read; no signer credential is created. The declared source
commit is recorded, not authenticated by the helper. Match it to reviewed build
and release provenance before any later execution.

The real `LedgerStore::record_payment` API records exactly 2,340 synthetic sats
and its atomic `payment_received` event. Reopening the database and repeating the
same source ID must return Duplicate with one event, unchanged credit, no feed
attempt and an empty Nostr outbox. This is a synthetic accounting seed, **not a
Strike settlement**, webhook or provider-recovery test. The existing real-daemon
gateway integration tests use the same ledger boundary. Provider tests and final
receive-only provider acceptance remain separate required evidence.

Existing destinations, dangling symlinks, aliased parents and SQLite URL delimiter
paths are rejected. Never clear existing state to rerun this helper. A failed
partial preparation remains evidence and requires review; select a fresh session
directory instead. `PREPARED.json` is emitted only after successful checks and
durable file writes. It explicitly records `started: false` and the open gates.
Do not copy a live SQLite main file without its WAL; use the reviewed quiesced
backup procedure when transferring this state to an installed runtime identity.

## Required activation handoff

Before installation, use the reviewed checkout's non-executing archive preflight:

```sh
python3 deploy/scripts/preflight-release.py /path/to/release.tar.gz \
  REVIEWED_FULL_SOURCE_SHA INDEPENDENTLY_REVIEWED_ARCHIVE_SHA256 > release-preflight.json
```

Both pins must come from reviewed build/release evidence. The digest is for the
inner release `.tar.gz`, not the GitHub artifact ZIP containing it. The command
requires Linux with `/proc` and a regular archive file; pass the real file path,
not a symlink. It rejects FIFOs, devices and directories before opening them for
I/O, and pins the checked inode across pathname replacement. It
checks a private snapshot against that digest before extraction, verifies every
payload checksum and the declared source, and reports per-file hashes. It does
not execute binaries or install files. Require a successful exit and valid JSON;
preserve failures, and do not treat an empty redirected output as a pass.
Checksums and a source declaration do not authenticate how binaries were built.
Keep the build's source/run provenance alongside this report. After installation,
compare actual binary/config hashes with this report and the separately reviewed
session configuration; verify ownership, effective permissions and sandbox
behavior independently. Installation must remain inactive until approval.

Preparation under a development identity is not installation acceptance. Before
launching a daemon against the home gateway, the reviewed session manifest must
bind the following to the same session/source:

- Verified release SHA and binary/config hashes; separate non-admin runtime
  identity, correct root-managed files, synthetic credential permissions and
  service sandbox. No production database or secret path is reused.
- HOME's acknowledged harmless rule/config/protocol and command-count evidence.
  The configured hostname/port alone does not prove a harmless target.
- Approved direct reciprocal peer mappings, both containment policies, independent
  recovery and closure of inspection exceptions. The live path has to be tested.
- Explicit operator approval covering network application and the harmless test.

This helper provides no start/apply command that bypasses those gates. The HOME
agent owns canary controls and authoritative per-delivery evidence; the VPS must
not infer delivery counts from its own ledger or a gateway request row.

## Cross-host acceptance sequence still to execute

Record HOME's initial counter and verify no preexisting unresolved canary request.
Keep the remote switch OFF while checking the fresh daemon's 2,340 credit and
correlated refusal/replay without any delivered command. After the approved
harmless enable step, observe the first command UUID. Duplicate and concurrent
requests while unresolved must not create another command. Restart and database
failure tests must keep polling that UUID without resending it.

Lost HTTP response and late owner completion are different cases. A VPS fault
proxy can hide an observation; it cannot establish that the owner completed late.
HOME must provide the acknowledged source-pinned harmless hold/release fixture
and every-command UUID/count evidence before that case is implemented or run.

Quiesce both services for paired backup/restore, preserve unresolved identity and
all HOME receipt/control state, and reset the restored overlay stream according
to the existing restore procedure. Never reset counts or discard unresolved
requests to make the test pass. When the first completion is authoritative, one
debit is allowed; after shipped cooldown the second confirmed command yields
exactly two command deliveries, two debits and 340 remaining sats. Replay,
restarts and settlement duplication must preserve those counts and balance.

Return the harmless remote switch OFF and close the home listener before removing
peer identity bindings. Keep both databases and receipts as evidence. These
cross-host results remain unproven until actually executed and independently
checked; local preparation success cannot close them.

Focused regression: `cargo test --locked --all-features --example prepare_cross_host`.
CI explicitly runs this example's tests in addition to the required full checks.

## Offline accounting comparison against the held fixture

The generation2 held fixture/control contract is source-reviewed at HOME PR77
`3b16b7a85e0a2ab88b42562c78a8b7dde728513f`. Its rule SHA256 is
`1cacb11569ab90db03bc0ee2f94fd3fec9d0e2132bc816d4f5982d4fd7d6a88c`.
HOME owns all OpenHAB reads/releases. The VPS does not receive its token or call
OpenHAB. The source-reviewed contract does not approve network exposure or a run.

`deploy/scripts/verify-cross-host-accounting.py` (Python 3.11+) compares captures
without network access or application writes. It consumes the exact JSON from
HOME's `control-held-canary.py` with no `--release`, once before the session and
once after completion/remote-OFF cleanup. Both captures must have Hold ON,
RemoteEnabled OFF, no unresolved delivery, and the pinned source digest. The HOME
helper checks Fault OFF before emitting them. Preserve its exit status and logs;
an unsigned capture is evidence to authenticate in the session handoff, not proof
of its own origin. Preserve the existing Count1 baseline; never reset the fixture.

After quiescing the session, supply the daemon's consistent exported database
(with matching WAL when applicable), the two HOME captures and the run UUID from
`PREPARED.json`:

```sh
python3 deploy/scripts/verify-cross-host-accounting.py \
  --database /private/session/export/daemon.db \
  --baseline /private/session/home-before.json \
  --completed /private/session/home-completed.json \
  --run-id ORIGINAL_PREPARED_RUN_UUID
```

Capture stdout, stderr and exit status to new evidence files. An empty output or
failed exit is not a pass. The verifier opens SQLite read-only, takes a consistent
read transaction and limits row/JSON sizes and SQLite work. It does not copy,
repair, migrate or initialize a database. Use only the synthetic session export;
this tool does not identify a live database or authenticate its provenance.

The comparison requires an unchanged complete baseline journal, exactly two new
owner deliveries with distinct nonhistorical UUIDs, and final Ack for the second
UUID. Every extra delivery fails, including a duplicate UUID whose ledger debit
was deduplicated. Those exact two UUIDs must match the daemon's confirmed attempts,
ordered 1,000-sat debits and ordered confirmation events with balances 1,340 then
340. The original run must have exactly one 2,340-sat synthetic payment/credit and
payment event; no unresolved attempt, public Nostr outbox or issued provider
request is permitted. Historical refusal attempts may remain as resolved
`reconciled_not_fed` records and may not match a delivered UUID.

A passing comparison establishes only consistency of the supplied captures. It
cannot prove that duplicates/concurrency, refusal cooldown, lost responses, late
release, process restarts or paired restore were actually exercised. Retain each
scenario's timestamps, request/response bytes, process/source pins, original UUID,
HOME before/after command journal, and backup/restore manifests separately. The
final authenticated path, actual unit sandbox, provider scopes, physical-owner
retention and operational approvals are still required. The launch/fault-control
portion of the cross-host harness remains to be integrated after HOME's held
binding and final staging path are reviewed.

Regression tests use the actual repository SQLite migrations plus captured
fixture-shaped data. They verify correct correlation and reject extra/duplicate
commands, changed baseline, wrong UUID/source, pending completion, inconsistent
financial events/debits, provider requests and public outbox work. These tests
are offline verifier tests, not a cross-host run.
