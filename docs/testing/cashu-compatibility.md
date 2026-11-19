# Cashu implementation checkpoint — 2026-11-15

> Historical dependency/intake checkpoint. For the current November 16 state,
> use [implementation status](../implementation-status.md), the
> [wallet boundary](../architecture/cashu-wallet-boundary.md), and the
> [development runtime](../deployment/cashu-wallet-development.md). The dated
> qualification ledger below records the initial increment; its pending claims
> do not describe the current implementation.

The later source now includes both intake paths, authenticated core receipt
import, financial feed planning and sweeps, actual-CDK combined settlement with
independent mocked Strike verification, lost-reply recovery, and the two-QR
frontend. Their source-pinned evidence is linked from implementation status.
Current metadata IPC/HTTP/snapshot qualification is in progress. Independent
sender/phone scans, coordinated restore, installed acceptance, fee/change
composition and production activation remain separate unfinished requirements.

Scope: [Nut November plan](../planning/cashu-nutnovember-implementation-plan.md).
This is a source-development checkpoint, not an installation or activation claim.

## Source selection

The original checkout was `6ac9363` on the September remediation branch, with
untracked operator tools and independent worktrees. It lacks the November credit,
cap, reward and relay-routing modules required by the plan. Development therefore
uses branch `feat/cashu-nutnovember-20261115`, isolated worktree
`.worktrees/cashu-nutnovember-20261115`, based on exactly
`54c053b1ae655c829fc6bed96ad59b22b409c082` (the plan's inspected baseline).
No existing branch, paid database, deployed service or operator tool was changed.
The later [November inventory](cashu-november-inventory.md) records the separately
selected pre-November first-parent reference and distinguishes inherited work.

## Dependency evaluation

The disposable package `tools/cashu-compat` pins CDK and cdk-sqlite **0.18.3**,
wallet-only features, with its own lockfile. The core manifest/lockfile, Rust 1.88,
SQLx 0.8.6 backport and application schemas remain unchanged. Package metadata
reports CDK's minimum Rust version as 1.85.0. Versioned upstream source is available
at <https://docs.rs/crate/cdk/0.18.3/source/> and
<https://docs.rs/crate/cdk-sqlite/0.18.3/source/>.

The initial probe constructs a wallet with a disposable synthetic seed and
`.invalid` mint, persists mint metadata and reopens the SQLite database. It does
not contact a mint, acquire proofs, exercise swaps, prove lost-response recovery,
or qualify SQLite for production custody. It never prints seed or token data.

**Storage qualification remains open:** the published adapter resolves rusqlite
0.31.0 and libsqlite3-sys 0.28.0, whereas the core deliberately requires
libsqlite3-sys 0.39.1 (see [SQLite backport](../security/sqlite-binding-backport.md)).
The compatibility package now carries the complete published rusqlite 0.31.0
archive/source, changing only its native dependency pin to exactly 0.39.1.
`verify_vendor.py` validates the archive digest, every file, the one permitted
manifest change and the locked binding checksum. Its negative tests reject changed
source, added/missing files, symlinks and a downgraded binding; the gate also runs
with Python optimization enabled. No upstream Rust/FFI source was edited.

The runtime probe and regression test require compiled SQLite 3.54.1. The
backport is a candidate until the Rust build and persistence tests pass; process
separation alone does not establish storage qualification. Do not promote the
compatibility probe into an installed wallet service.

The matching cdk-redb 0.18.3 package and rusqlite 0.41.0 both declare Rust
**1.89.0**, so neither is a drop-in option under the preserved 1.88 toolchain.
This is why the minimal native-binding backport is being evaluated.

Upstream 0.18.3 has persistent receive/swap/melt saga and recovery APIs. Inspecting
those APIs is not qualification: interruption fixtures must establish ownership of
restored outputs, exact request recovery and conservation before any receipt import.
A process-local retry alone cannot establish cross-store idempotency.

## Qualification ledger

| Requirement | Current evidence / next work |
| --- | --- |
| Reproducible CDK build on Rust 1.88 | Locked wallet tests passed: 19, none skipped; constructor probe execution remains pending |
| Recoverable production wallet storage | Backport compiles; engine-version, reservation/counter reopen and corruption tests pass; full saga/custody recovery qualification remains open |
| Two configured mints, fees, keysets and exit capability | Not yet qualified; no production mint selected |
| Ordinary NUT-18 and NIP-61 acquisition | Common coordinator implemented; full protocol fixtures pending; public workers and interoperability unfinished |
| Cross-transport deduplication and atomic core import | Shared intake journal prepared; tests pending; core import not implemented |
| Treasury purpose, source allocations and feed gate | Not implemented |
| Two QR overlay and 1080p/720p collision/decode tests | Not implemented |
| Real independent-client/phone scans | Not performed; record separately from fixtures |
| Recovery/backup, inactive units and acceptance suites | Not implemented |
| Cashu-derived reward authorization | Disabled / unqualified |
| Installed / financially enabled / live verified | None |

No mint URLs, production limits, keys, live payment amounts or physical actions
are selected by this checkpoint. Receiving, discovery, receipt import, settlement
spending and Cashu-derived rewards need separate default-off controls.

## Checks at this checkpoint

- Core `cargo fmt --all --check`: passed after shared-verifier extraction.
- Probe `cargo fmt --manifest-path tools/cashu-compat/Cargo.toml --check`: passed.
- `git diff --check`: passed.
- Initial probe builds: stopped for resource tuning or failed (full shared `/tmp` and later API/import errors); none is a passing runtime result.
- Exact-source backport gate and its tampering tests: passed.
- Corrected patched build and tests: passed, 16 library + 3 storage/binary tests, none skipped.
- Clippy, full core tests and protocol/recovery acceptance: pending.
- Wallet dependency audit passed at its recorded checkpoint; recheck after final dependency/source changes.

The compatibility package is outside the core workspace deliberately. Its checks
must be invoked with `--manifest-path tools/cashu-compat/Cargo.toml`; root
`--all-features` does not exercise it.

## Shared intake journal under development

`tools/cashu-compat/src/journal.rs` implements the first wallet-side durable
boundary, still outside production and not reachable over HTTP, relay or IPC.
Its caller must perform mint-policy/cryptographic validation before admission;
this module is not a signature, DLEQ or mint-settlement verifier.

The index uses CDK's proof Y plus configured canonical mint identity and sat unit.
Keyset, amount and signature evidence are immutable. Entire overlapping proof
sets, duplicate entries, conflicting source IDs and overflowing amounts reject
transactionally. Cross-transport copies retain the original acquisition and
attribution. The same epoch, prepared request, proof index and credit-ack record
survive reopen. The receipt export contains no spendable proof material and can
be retried after acknowledgment; it does not yet create core credit.

A request is frozen before the acquiring transition. Reopening an acquiring job
exposes the original request through a private recovery API and never starts a
new acquisition automatically. Net acquisition plus receive fee must equal the
original gross. Acknowledgments and acquired evidence cannot be overwritten by
conflicting retries. These contracts have regression tests prepared; passing
runtime evidence is still pending compilation.

The local epoch mismatch fence detects an externally supplied epoch mismatch;
it cannot detect restoring both the database and its epoch authority together.
Coordinated backup manifests, external-state reconciliation, admission/rate/exposure
budgets, full operation holds, CDK saga binding, validated intake, peer-authenticated
receipt import and restore fencing remain required before receiving funds.

The initial debug build was deliberately stopped after observed heavy swapping
in the Lightning dependency. The replacement uses one compiler job and disables
debug symbols for that dependency. The replacement then failed writing Lightning metadata because the shared `/tmp`
tmpfs had only 31 MiB available. The next build uses an ignored on-disk
`tools/cashu-compat/target/build-tmp` directory via `TMPDIR` and one compiler job.
That build remains live; no passing Rust result is claimed. All protocol, core
and deployed acceptance remains open.

## Mint policy boundary prepared

`mint_policy.rs` accepts only configured identities and HTTPS mint URLs. It
rejects credentials, queries, fragments, unconfigured aliases and duplicate issuer
identities. Ordinary and nutzap receiving gates are separate, with a required
qualified BOLT11 exit. Two fixture policies require no provider-specific source
change. These are development policies, not selected production mints.

Connection-address validation rejects empty/oversized answer sets and any mixed
public/private set, including IPv4-mapped IPv6, metadata, loopback, link-local,
ULA, documentation, transition and special-use ranges. Tests cover IPv4/IPv6,
unknown URL/alias admission and changed answers between connection attempts.
The address validator is **not yet a network connector**: transport integration
must still pin its approved answers, preserve TLS hostname checks, disable
redirects/proxies and apply time/body limits on every actual mint request. This
module does not yet establish complete SSRF or DNS-rebinding acceptance.

## Constrained CDK transport prepared

`transport.rs` implements CDK's pluggable HTTP transport and `wallet.rs` installs
it in the wallet builder. Requests are bound to one configured mint origin and
base path, with a receive/recovery/BOLT11-melt route allowlist. Generic minting,
LNURL destinations, form/OIDC requests, authentication profiles, proxies, insecure
TLS and WebSockets are not enabled by this adapter. Unsupported optional mint
authentication is a compatibility restriction to record in the mint matrix.

Every HTTP operation builds a no-proxy/no-redirect HTTPS client with freshly
validated and pinned DNS answers. The outer timeout includes DNS and response
reading; request serialization and streamed responses have configurable bounds.
Provider errors and parse failures return coarse messages rather than request,
response or token material. This adapter is prepared source with unverified Rust
runtime tests; it is not yet a fully qualified network boundary. Actual TLS,
redirect, response-stream and independent mint acceptance fixtures remain needed.

The offline construction probe now uses this builder rather than CDK's default
network client. That probe still has both receiving paths disabled and no public
listener. A successful constructor must not be described as a successful Cashu
receive.

Recovery integration must not trust CDK's report or transaction status alone:
0.18.3 `receive/saga/resume.rs::recover_or_compensate_receive` has a branch that
marks an operation completed when its reserved proof set is empty. The common
engine must bind the original request, resulting wallet outputs and actual
ownership evidence, and hold missing/corrupt/rolled-back outputs even if a saga
or transaction says completed. No receipt import currently uses that shortcut.

## Receiving descriptor source prepared

`descriptor.rs` uses the pinned CDK Cashu protocol types for reusable NUT-18
requests and NUT-26 encoding. It has no fixed amount, uses sat units, a strict
nonempty configured/qualified mint set, a BOLT11 capability hint and one HTTPS
POST transport. It contains receive instructions, never spendable token proofs.

The immutable descriptor ID hashes the mint/policy set, transport and receiving
key; a new donation does not rotate it. CreqA bytes retain case. Both encodings
have round-trip and semantic test cases prepared. No encoding is qualified for
display until independent wallet and actual phone/video scan evidence is recorded.
The source references were rechecked at <https://cashubtc.github.io/nuts/18/> and
<https://nips.nostr.com/61>; protocol revision pinning and client matrix remain open.
No HTTP ingress or nutzap relay worker is activated by descriptor construction.

## Dependency audit follow-up

The wallet graph's inverse `rsa` lookup found no package. An audit of its new
lockfile found quinn-proto 0.11.18 (RUSTSEC-2026-0523/0524) and quinn-udp 0.5.15
(RUSTSEC-2026-0538), retained as optional HTTP-stack dependencies. Targeted locked
updates now select quinn-proto 0.11.19 and quinn-udp 0.5.16. The core graph is
unchanged. The audit recheck passed with zero reported vulnerabilities against 1509 loaded
advisories. It retains one allowed `paste` 1.0.15 unmaintained warning
(RUSTSEC-2024-0436). The second recheck had no package-cache/index access warning.
No blanket advisory suppression was added.

## Shared verifier and CDK API compatibility

The bounded NIP-01 verifier and signed-event type now live in
`crates/nostr-event`. Core `nostr` and `nostr_event` retain their existing public
paths through reexports, and all original verifier regression fixtures remain
in the core. The core lockfile only adds this local package; existing registry
versions remain unchanged. Full core tests are required before accepting the
extraction. Wallet intake uses exactly the same verifier rather than a fork.

The first type-check exposed CDK 0.18.3's actual public API: the database module
is `cdk_database`, proof types are under `wallet::types`, and the generic wallet
HTTP client is hidden behind a private module. The vendored CDK archive now adds
one public reexport, `CustomHttpClient`, so our transport can use the original
connector and its recovery behavior. No upstream crypto, request or recovery
implementation changes. The provenance gate verifies this exact addition and
all untouched source bytes against archive SHA256
`6969a2ea499035f5f49f5cdaf355f34251ce85a8ddd47c557177ecee5ed7da2a`.

## Common intake validation prepared

`intake.rs` handles bounded ordinary NUT-18 payloads and signed kind-9321 events
through the same pinned-keyset/DLEQ/condition validation. It checks recipient,
sat unit, configured mint, exact NIP-61 advertised URL and proof count/value
before journal admission. Only simple receiver P2PK is accepted for nutzaps;
refunds, timelocks, additional signers, signature flags, unknown locking fields
and blinded locking profiles are rejected. Receiver keys are compared using the
library's x-only key handling; NIP-61 additionally requires even-parity encoding.

Private bearer proofs may omit DLEQ under this explicit policy, with no receipt
or core credit until successful swap and owned-output persistence. Private P2PK
proofs require DLEQ before admission, preventing a public locked proof from being
rewrapped without its evidence to claim an identity. Supplied invalid DLEQ always
rejects. The engine still needs definitive-invalid-operation handling and bounded
admission resources before public receiving.

Prepared crypto fixtures use the library's actual blind/sign/unblind/DLEQ
operations and real Schnorr event signatures. They cover cross-transport replay,
multiple payments to one descriptor, forged events, malformed proof evidence,
unsupported lock branches and no pre-acquisition receipt export. All 19 wallet tests passed. They are fixture coverage, not independent-wallet or
real-mint acceptance.

The 19-test run completed successfully on Rust 1.88 with the constrained transport,
shared Nostr verifier and both source backports. It does not yet test a real mint
swap, interrupted swap recovery, core receipt import, Strike treasury settlement,
public HTTP/relay workers, physical dispatch or rendered QR scans. Strict Clippy
and the core suite are the next checks, not inferred successes from these tests.

## Write-ahead operation guard and process ownership prepared

`operation_gate.rs` wraps the original CDK HTTP connector. A caller must arm an
admitted operation under a retained exclusive writer lock. Exact swap bytes are
committed before submission; successful response bytes are persisted before CDK
processes them. These are private recovery records, never financial receipts.
Replays require the original ID and exact request. A replacement request, unbound
swap, melt or concurrent operation is rejected. Lost replies remain acquiring;
a fresh request is not inferred from timeout. An unresolved operation blocks new
work at its own mint while another configured mint can still be selected.

`writer_lock.rs` uses a nonblocking kernel file lock with a runtime-owned private
directory and single-link, no-symlink private file. Journal and wallet-store open
helpers create private files and reject unsafe existing permissions. No lease
expires while a slow/uncertain mint operation might still exist. All admitted
financial work must retain this lock; the standalone offline constructor is not
a receiving service and has no listener or active receiving flags.

Journal holds retain the original request and proof claims. They cannot export
credit or be restarted as fresh acquisitions, and they cannot silently rewind a
receipt already exported to the core. Original hold reasons survive reopen.

New tests cover request-before-call persistence, execution with lost reply,
exact cached recovery, no new credit from a successful HTTP reply, per-mint
uncertainty, same-directory writer exclusion and unsafe path rejection. They
are queued behind the core Clippy check and not yet counted as passing. The prior
19-test wallet slice passed strict Clippy; the new guard/hold changes still need
their own test and Clippy results. Full acquired-output reconciliation and the
receiving workers remain unfinished.

The operation gate now also verifies the actual CDK swap input Ys, keysets,
amounts and signatures against the admitted acquisition before recording or
submitting it. Witness/DLEQ transport changes do not create new identities.
Empty/zero-valued or value-increasing output sets reject before the mint call.
Full receive-fee authority and owned-output reconciliation remain unfinished.

The core Clippy pass found a test-only private import shadowing the new public
signed-event reexport. The redundant import has been removed; the observed
run still needs to finish and be rerun on that correction. It is not a passing
core Clippy result. Wallet guard tests remain queued on the shared build lock.

## Admission recovery material and receive-fee authority

Original proof material and validated nutzap author context now commit atomically
with the admission/proof index, in private `acquisition_context` rows. Recovery
can reload the exact original proofs after restart. Rewrapping through another
transport does not overwrite the original author or source. Ordinary Cashu is
unattributed. Missing retained context prevents receipt export rather than
reconstructing a financial claim from a balance or relay event. Wallet backups
must include these rows as well as CDK state, keys and cross-store acknowledgments.

Arming a swap now requires an explicit maximum receive fee. The actual input minus
output value must fit that authority before request persistence/submission.
Zero permits only zero-fee swaps; no production fee ceiling is selected here.
Tests are prepared for fee rejection without submission and original-context
recovery/replay. These additions are not yet part of the earlier 19-test result.

The obsolete core Clippy run was stopped after its already-reported fatal warning
and source correction. Current wallet tests are now compiling; corrected core
Clippy is queued separately. Neither current result is claimed as passed.

## Owned-output verification prepared

`ownership.rs` captures the actual CDK receive saga's operation ID, mint, keyset,
NUT-13 counter range and exact blinded outputs before submission. The production
operation gate now requires its CDK database, finds exactly one matching prepared
receive saga and persists this derivation binding with the original request.
A missing or ambiguous binding rejects the mint call. Isolated transport-boundary
unit fixtures bypass only this database lookup; they cannot produce an ownership
attestation.

The verifier regenerates expected output secrets/blinded messages through CDK's
NUT-13 implementation, requires matching persisted wallet proofs, validates DLEQ
and blinding evidence against qualified mint keys, and checks exact output value.
It requires fresh unspent-state evidence for every resulting output, rejecting
missing, duplicate, spent, wrong-seed or corrupt outputs. It does not use a wallet
balance delta or a Completed/Recovered label as settlement evidence. Only a sealed
owned-acquisition result can call the nonpublic acquired-state transition.

Its crypto fixture and worker integration are not yet qualified. Output DLEQ is
required for this recovery profile; selected mints must be qualified for it before
receiving, including ordinary payments whose input DLEQ may be absent.

The subsequent 23-library-test run failed: 11 passed and 12 rejected the fixture
state directories' permissions. The default temporary-directory mode was too
broad for the new runtime checks. Fixtures and the disposable probe now explicitly
create 0700 directories; runtime checks were retained. The storage/binary suite
was not reached in that failed run. The corrected tests, including ownership
verification, are queued behind corrected core Clippy. No new passing result is
claimed from this failure.

All connectors holding the same writer lock now share one in-process operation
coordinator, including independently constructed connectors for different mints.
A test checks this rather than relying only on cloning one connector. Pending
financial work remains separate from the physical feeder-attempt state.

## Common acquisition coordinator prepared

`acquisition.rs` now connects durable admission, the gate-protected CDK receive,
owned-output reconciliation and receipt export. Admission, acquisition, receipt
export and original-swap replay are separate default-off permissions. It borrows
the protected seed, retains runtime writer coordination, supplies only the dedicated
receiving key for P2PK inputs, and exposes no core/Strike/signer/feeder authority.

New submission reads the persisted original inputs and refuses to start another
swap for an already submitted acquisition or while CDK has unresolved mint work.
The production gate checks each requested output denomination/keyset against
qualified keys before contacting the mint, including a keyset selected after a
CDK inactive-keyset retry. Unqualified rotations cannot silently acquire funds.

Reconciliation derives expected Ys, reads the actual stored outputs, asks the
configured mint for fresh proof states and seals the result through ownership
verification. Unavailable state reads remain pending; conflicting ownership is
held. A duplicate after acquisition/export/ack returns the original receipt state
without requiring the already-spent wallet outputs to remain unspent.

Original swap replay needs its separate permission and exactly one matching CDK
saga. It always uses the frozen request gate, then rechecks ownership; CDK's
recovery report does not itself produce credit. Recovery of the crash gap before
the app's derivation hook, below-threshold spending/sweeps, restore fencing and
full protocol-boundary fixture qualification remain unfinished.

Corrected core Clippy completed successfully (`--locked --all-targets
--all-features -- -D warnings`). The core full test suite remains required. The
current wallet tests are compiling after the private-directory fixture correction;
this coordinator is not yet covered by a passing full swap/restart fixture.

## Current verification checkpoint

The corrected wallet run passed **25 library tests plus 3 binary/storage tests**,
with zero failures and zero ignored tests. Its dependency file includes
`src/acquisition.rs`, so the coordinator compiled in this run, while its full
network-protocol swap/restart behavior remains untested. The run covers the
ownership crypto fixture, configured-gate rejection without a durable CDK saga,
original-author preservation, receive-fee limits, holds and writer exclusion.

Core strict Clippy passed on the corrected shared verifier. Strict wallet Clippy
is now running on the coordinator/ownership slice. The root full test suite,
full protocol-mint fixtures, independent clients, receipt IPC/core credit import,
Strike treasury funding, feed-source selection, edge/relay workers, two-QR browser
acceptance and operational qualification remain required. None is implied by
these 28 passing wallet tests.

Wallet strict Clippy subsequently flagged the coordinator's large receipt enum
variant. That variant now stores a boxed receipt; the recheck is queued and no
passing result is inferred from the size fix. The required core `cargo test
--locked --all-features` run has started with one build job, on-disk temporary
files and test debug symbols disabled to fit the build host. Only completed,
task-owned wallet test executables were removed to reserve disk; no database,
source, key material or unrelated artifact was removed.

## Full CDK coordinator protocol fixture prepared

`acquisition/tests.rs` implements a synthetic in-process mint transport with
per-denomination signing keys, canonical keyset IDs, signature/P2PK validation,
atomic spent-input tracking, cached exact swap replies and NUT-09 restoration.
The actual pinned CDK wallet, SQLite stores, operation gate, ownership verifier
and common intake validators run against this transport. It opens no sockets,
uses no production keys and does not represent independent-client/live evidence.

Prepared cases acquire ordinary Cashu and a signed NIP-61 receiver-P2PK payment,
reuse one descriptor, replay the nutzap over HTTP without another acquisition,
restart after an executed swap with a lost reply and expired response cache,
conserve receive fees, and defer receipt export while that toggle is disabled.
A negative case removes the disposable wallet's outputs after a completed CDK
transaction and requires no receipt or second mint execution. This is fault
injection, not an operator recovery instruction.

These protocol cases have not run yet; they are queued behind the existing core
tests and wallet Clippy. Disk pressure during the core build was relieved only
by deleting obsolete check-only metadata from this task's target directory;
source, databases, keys, active build inputs and unrelated artifacts were retained.

## Private receipt wire shape prepared

`receipt_wire.rs` defines a bounded, versioned, immutable receipt envelope binding
an explicitly supplied wallet instance, epoch, acquisition, original source,
validated nutzap author, canonical internal mint/policy, sat unit, herd pool,
amounts/receive fees and acquisition-evidence digest. It contains no proofs or
spendable output material. Structural validation rejects unknown fields, bad
identities/units/destinations, overflow/conservation errors and ordinary-Cashu
Nostr attribution. Canonical digesting is independent of JSON field order and
whitespace and binds all receipt fields for conflict detection.

The envelope is not a financial capability: core import must still authenticate
the peer UID/wallet registration and epoch, enforce exposure/credit limits and
atomically create its own inbox/provenance/credit/event records. No endpoint or
core grant is enabled by this module. Its tests are prepared and not yet passed;
the wallet protocol run was queued before this addition and must be checked for
the actual source files/cases it includes.

## Receipt authority checks prepared

`receipt_authority.rs` obtains sender credentials from the actual Unix socket,
rejects root/same-runtime/unregistered senders before parsing, and pins the wallet
instance and epoch. There is no constructor from serialized UID claims. A peer
receipt also retains the authenticated UID so revoking/changing the registration
cannot reuse an earlier token under another identity.

New-import checks are separate from authenticated duplicate/ack lookup. They
require an approved historical mint/policy pair, explicit per-receipt gross/net
limits and remaining receipt-count, net-credit and custody budgets. Zero/unknown
budgets fail closed. The core transaction must read and consume these budgets,
perform exact digest replay/conflict lookup, and atomically create provenance,
credit/cap retirement and its public event; these helpers do not reserve budget
or grant credit. Actual distinct-UID server qualification remains open. Prepared
tests include pure peer-policy boundaries and real same-UID socket rejection.

Integration inspection confirms that existing physical dispatch currently uses
aggregate credit, while reward funding debits consume FIFO lots. Cashu credit
must therefore remain unavailable to production import until the separate
financial reservation/eligible-lot selection and confirmation debit agree on the
same frozen sources. Merely adding a new source string or an unavailable reward
lot would not satisfy the plan. Existing reward funding is initialized explicitly;
Cashu implementation must not silently reinitialize paid funding history.

Additional disk pressure required evicting wallet-only codegen cache entries
identified by comparing the wallet and core lockfiles. The active core tests do
not depend on those packages. Waiting wallet verification commands were stopped
and requeued with debug symbols disabled and one build job, retaining their full
strict-Clippy/all-feature test scope. The core run was left active. These cache
and profile changes are resource management, not passing test evidence.


## Core receipt import and source gate prepared (2026-11-15)

The receipt schema now lives in `crates/cashu-receipt`; the wallet projection
reuses it without introducing CDK into the core. Kernel peer/registration checks
moved into `src/cashu/receipt_authority.rs`. A separate core epoch is pinned by
protected policy. The actual-CDK nutzap fixture now uses different synthetic
project recipient, receiving-P2PK and sender identities.

The new core importer atomically performs receipt replay/conflict lookup,
clock/rate/custody checks, private provenance, existing capped credit/retirement,
and one public payment event. Source history must already be initialized
explicitly. Existing native Strike grants/paid rows are untouched. Cashu sources
retain zero existing Strike payout authorization.

After the first Cashu import, physical intent selects eligible non-Cashu source
lots, freezes them, and uses those same sources for confirmation, attribution and
reward evidence. Unsettled Cashu is skipped rather than occupying a physical
attempt. This is a dispatch safeguard and later-Lightning progress path, not the
required complete treasury implementation. Cashu coverage, financial reservations,
BOLT11 melts, verified Strike transfers and their fee/change reconciliation remain
unimplemented.

Migration 26 adds only empty private receipt/clock/source-allocation tables.
Current maintenance expects schema 26. Historical contact/cap/fee comparisons
retain explicit schema-25 targets; a separate inactive 25-to-26 Cashu comparison
and source-snapshot commands are added. Their existing deployment automation is
not yet qualified to install this candidate. Four-journal verification explicitly
refuses Cashu-bearing state pending the required coordinated wallet/core archive.
See [the boundary document](../architecture/cashu-wallet-boundary.md).

Prepared core financial tests cover concurrent same-receipt imports, changed
receipt/evidence conflicts, immutable acknowledgments after cap retirement and
restart with import disabled, late transactional failure, zero/rate/custody
budgets, clock/epoch fences, a 600-Lightning/250-nutzap/150-Cashu shared balance
that cannot dispatch before settlement, and later native sources progressing
without consuming earlier held Cashu. Maintenance fixtures compare retained paid,
signed and pending state and reject changed rows, unexpected triggers or activated
new tables. These new cases are awaiting their actual results.

### Verification evidence from this iteration

- The earlier full core Cargo test invocation failed 102 library cases in the
  restricted environment. A targeted socket case exposed both sandbox socket
  denial and an overlong temporary path. It passed outside those restrictions
  using a short task-private on-disk `TMPDIR`.
- The already-built core library suite then passed **449**, failed **0**, and
  retained **6 explicitly ignored** tests. This was before the receipt-import and
  schema-26 changes; it is not evidence for the new financial code or the full
  Cargo all-feature suite.
- Wallet strict Clippy reached application code but had loaded the earlier
  manifest before receipt extraction; it failed on the new unresolved shared
  crate. A fresh invocation is queued with the current local dependency graph.
- Expanded wallet tests stopped with **ENOSPC** while compiling Bitcoin. This is
  not a passing or skipped test result. Obsolete task-only executable caches were
  removed, freeing about 0.9 GB of logical build artifacts. Source, databases,
  keys and unrelated worktrees were retained. Expanded tests still need rerunning.
- Current exact vendor verification and its two negative-mutation test groups
  passed. Both cached `cargo audit --no-fetch` scans exited successfully against
  1509 local advisories; the wallet retains the documented `paste` maintenance
  warning. Both warned that the sandbox could not lock the crates.io index.
  This is cached-advisory evidence, not a claim of a fresh advisory update.
- `cargo tree -i rsa --offline --locked` found no active core dependency.
- Fresh core strict Clippy, wallet strict Clippy and targeted core Cashu tests
  remain live/queued. No current source-complete or required-gate claim is made.

Root, wallet and shared-receipt formatting checks and `git diff --check` pass.
The fresh core Clippy run reported a manual-range-pattern lint in the expanded
schema-prefix check; it has been corrected to `23..=25`. The original invocation
is still live and its terminal result must be recorded, followed by a fresh check.


## Acquisition/import fixtures now passed; treasury source in progress

The refreshed wallet run completed with **31 library tests plus 3 binary/storage
tests passed**, zero failures/ignored. This includes actual CDK receive/swap,
receiver P2PK witness enforcement in the synthetic issuer, ordinary and signed
NIP-61 acquisition through one engine, cross-transport replay, owned-output
checks, receive fees and restart/restore after an executed swap with lost reply
and expired cached response. These are local synthetic protocol fixtures, not
independent-client or live-mint qualification. The preceding wallet all-target /
all-feature strict Clippy run passed; later shared treasury additions require a
fresh final check.

The six new core Cashu import/source-selection tests also passed, before treasury
extensions. The already-built maintenance checkpoint ran **28 passed, 1 failed,
3 explicitly ignored**. The failure exposed that the new schema-25 comparison
called a prefix digest still limited to 8/23/24. Source now explicitly admits
25 as well; that correction needs a new run. Historical contact, cap and fee
comparison fixtures passed in that checkpoint.

Treasury types and core ledger source now add immutable purpose/source budgets,
per-mint/global bounds, a never-expiring invoice-creation claim, exact invoice
binding, independently verified native-BTC Strike receive evidence, separately
authenticated wallet fee/owned-change reports, and atomic completion/source
coverage. Both inputs are required for coverage. Recovered change stays custody;
conversion fees do not reduce original credit. Treasury receives are excluded
from ordinary contributions, caps, celebrations and native Strike reward proof.
Known original creation results can be adopted by read-only provider lookup,
including while new spending is off; no retry creates a replacement invoice.
The receive adapter reuses its BTC request and authoritative reconciliation code.

Five new loopback provider/core fixtures are prepared: mixed three-rail zero-fee
funding, nonzero actual fees/change with unchanged original credit, callback-before-
wallet-response ordering, original-invoice adoption, and conflicting/pending/
wrong-scope results. They do not execute a real Cashu melt or physical feed.
Their compile/run result remains pending. The treasury library-only type check
(`cargo check --offline --locked --lib`, handle 12825) passed in 19m33s. This does
not compile or qualify the new test fixtures or complete the required gates.
The Cashu migration is still unreleased; its source now includes the treasury
journals as part of the same additive schema-26 boundary.

The wallet melt operation journal/ownership implementation, proposed-feeding
financial plans, autonomous settlement/sweep worker, private IPC/service wiring,
coordinated backups, independently qualified senders/mints, overlay and deployment
qualification remain open. No source-complete, installed or live Cashu claim is
made. Completed obsolete task executables/old core codegen were removed only
from this worktree's ignored target cache to preserve host disk headroom.

## Disk cleanup and urgent CyberHerd hotfix checkpoint

All previously recorded compiler/test handles are terminal. The old core strict
Clippy invocation failed on the range-pattern lint described above; its source
correction still needs a fresh strict gate. The completed acquisition/import
checks and library-only treasury type check above retain their limited scope.

At the operator's request, disposable completed build caches, 44 clean inactive
worktrees and inspected obsolete temporary build directories were removed.
Their Git branch refs/commits remain. This dirty Cashu worktree, uncommitted source
and production/backup state were preserved. About 14 GB was recovered before
the separate CyberHerd hotfix's bounded shared verification cache began.
The old executables and deleted target handle metadata are no longer available;
they must not be treated as runnable current qualification evidence.

The missing-profile CyberHerd queue correction is isolated on
`fix/cyberherd-intake-liveness-20261115`, based on the installed pre-Cashu source.
It does not activate or qualify Cashu. The full Cashu implementation plan remains
active and incomplete, with the open wallet melt/IPC/backup/operational work listed
above.

## Deterministic allocation and wallet invoice preflight

The shared treasury report now enforces deterministic source allocation: consume
frozen sources in ledger order for deposit, then actual fees, retaining change on
its original source. Balanced reports that shift deposit coverage between donors
are rejected. Core provider fixtures use the same allocator. New tests cover
reassignment despite balanced equations, all small fee/change partitions, invalid
conservation and oversized fees/change. Targeted shared-crate tests (handle 38449) passed: two tests, zero failures
or ignored cases. This qualifies allocation arithmetic and rejection only.

The wallet's new treasury preflight checks protected destination/mint/policy and
core/wallet epochs, signed mainnet BOLT11 amount/hash, bound receive identity and
local creation/expiry bounds before new quote work. It does not authenticate IPC,
reserve proofs, execute melts or infer failed payments from expiry. Those paths
remain to be implemented. Wallet library type-check handle 63840 is queued against
the same compact shared target; this new module is not yet qualified. No
production service or financial permission changed.

The wallet treasury preflight now has a signed BOLT11 regression fixture covering
mainnet acceptance, testnet rejection, exact millisatoshi mismatch, changed hash,
future/expired timestamps including equality at expiry, excessive invoice life,
core/wallet epoch changes, wallet instance, destination account/credential,
mint/policy changes and missing receive identity. Test handle 2772 is queued
behind the still-running wallet library check 63840, sharing one target cache.
Neither result is yet known. The protected-peer IPC boundary and durable melt
operation journal remain required before this validator can participate in a
running wallet service.

Wallet source now adds immutable treasury-instruction admission in the same
WAL/FULL journal as intake. It pins the wallet epoch and protected instruction
scope, validates new signed invoices before insertion, bounds unresolved
instruction admission at 1,024, and makes receive IDs/payment hashes unique.
Exact original replay survives local expiry; changed invoice bytes or another
intent claiming the original receive/hash fail. New restart/conflict/epoch and
expired-first-admission cases are included in the queued treasury tests (2772).
The library check 63840 remains live, compiling dependencies; these new journal
cases have not yet run. Admission neither reserves proofs nor authorizes mint
POSTs; the existing operation transport still rejects melt requests. Durable
proof allocation, prepared CDK melt binding and authenticated IPC remain open.

Treasury source-value holds now commit atomically in the wallet journal. Each
source must match the original acquired receipt digest, mint and wallet scope;
aggregate held value cannot exceed acquired net value. Exact retries validate
the retained holds, including after expiry, and no expiry-based release exists.
A read-only original-instruction getter supports recovery. The new synthetic
journal test reopens a held source and rejects a second independently signed
invoice attempting to reserve that same value. It is included by the queued
`treasury::tests` filter. Exact CDK proof selection/reservation and terminal
change accounting are still required; these value holds alone cannot execute a
melt. The current transport continues rejecting melt POSTs.

Wallet library type-check handle 63840 passed (5m56s), including treasury invoice
validation and source-value reservations as loaded at that checkpoint. Treasury
regression handle 2772 remains live. Subsequent source adds an owned-output
attribution index: verified acquisition output Ys, signatures and amounts commit
atomically with acquisition completion and the receipt digest. A mint/output Y
cannot be assigned to two acquisitions. No secrets are added to the public
receipt. The new rollback/reopen/double-claim test is prepared, but not included
by the existing treasury-only filter and still requires running. These source
changes require a fresh final type/Clippy gate. Exact treasury proof reservation,
change ownership and melt execution remain open.

Exact treasury proof-ID reservation now freezes a bounded canonical selection of
owned output Ys per source. Every selected Y must belong to its acquired source
and mint; selected denominations must equal its already-reserved source value.
Unique mint/Y holds prevent concurrent selection. The selection and all holds
commit atomically; late sum mismatch rolls back prior inserts. Exact selection
replay survives expiry/reopen; a changed selection fails. The treasury journal
fixture now covers incomplete denomination rollback and restart replay. These
cases remain queued in live test handle 2772 and full library handle 50986.
Fresh CDK ownership/DLEQ/state verification, denomination splitting for partial
sources, quote binding, melt submission and recovered-change accounting remain
required. No timeout or expiry path releases proof/value holds.

The treasury journal now binds one original unpaid BOLT11 quote per intent and
one intent per mint/quote ID. The quote must match the exact invoice, configured
qualified mint URL, sat unit, deposit amount and bounded fee reserve; source proof
selection must already be retained. New expired, pending/paid, previously used
or non-Lightning terms fail. Exact frozen replay remains available after expiry,
without treating expiry as nonpayment or authorizing resubmission. Synthetic
quote mutations and restart/replacement cases are added to the treasury fixture.
Both test handles (2772 and 50986) remain live; the fresh quote code has no passing
result yet. Total input/keyset fees, CDK prepared-saga capture, submission gating
and final change ownership remain to implement before any execution is possible.

Handles 2772 and 50986 became unavailable before a terminal result was retained;
no passing result is inferred. The full wallet library suite was restarted as
handle 60596 with output retained at `/tmp/lg-cashu-wallet-tests-current.log`.
It is compiling CDK at this checkpoint. The restart follows missing authoritative
handles, not an observation timeout.

New source binds an actual CDK PreparedMelt to the retained quote, exact held
output Ys/signatures/amounts, original operation UUID and total fee authority
(input fees plus Lightning fee reserve). The intended confirmation mode is
skip_swap, preserving selected proof provenance; implicit swaps are not permitted
by this binding and require a separately journaled splitting workflow. The
operation/terms freeze atomically with prepared state and exact replay remains
stable. This source still needs actual PreparedMelt fixture qualification. It
does not authorize any transport POST: final request/change derivation freezing,
current DLEQ/state verification and execution/reconciliation remain required.

Wallet treasury verification checkpoint: retained handle 60596 completed with
35 library tests passed, zero failed/ignored. The new real-CDK prepared-melt
fixture additionally acquires synthetic cryptographically verified ecash through
the acquisition coordinator, reserves its exact owned outputs, binds the original
invoice and quote, and calls CDK `prepare_melt_proofs`. It binds that actual
operation, reopens the journal, accepts the identical operation after expiry and
rejects a different wallet epoch. The targeted fixture passed (one test). No melt
request is permitted by the fixture transport. This verifies preparation only,
not execution, change recovery, or independent provider settlement.

Strict wallet Clippy initially found a test-module ordering violation; the module
was moved after the implementation. A fresh locked/offline all-targets,
all-features Clippy run with warnings denied passed (8.99s). Wallet format and
patch whitespace checks passed. The full wallet all-features suite is being
rerun with retained output in `/tmp/lg-cashu-wallet-full-current.log`; root gates
and end-to-end settlement qualification remain outstanding.

Full wallet suite handle 62463 subsequently completed successfully: 36 library
and three binary tests passed, zero failures/ignored; doc tests passed (zero
cases). This supersedes the pending full-wallet result above. It does not replace
the separate root workspace gates or the outstanding settlement/recovery tests.

Melt change-derivation validation checkpoint: full locked/offline wallet
all-features suite passed with 38 library and three binary tests, zero failures
or ignored tests, plus doc tests (zero cases). Added synthetic recovery-material
fixtures cover zero/nonzero change, wrong seed, duplicate inputs, missing or
altered final Ys, missing/conflicting counters, changed quote and change amount,
missing outputs, unaffordable reserve and changed input fees. These exercise
`MeltDerivation::capture`; they do not prove a paid melt, mint interoperability,
or durable pre-POST integration. Wallet formatting and whitespace checks pass.
Fresh wallet all-targets/all-features strict Clippy also passed (11.28s).
Root workspace gates and the remaining full-plan qualification remain pending.

Durable melt-request checkpoint: the full locked/offline wallet suite passes
again (38 library + three binary tests, zero failures/ignored). The existing
real-acquisition/prepared-melt fixture now models the pre-POST saga transition,
then verifies first-freeze expiry rejection, successful atomic request/derivation
storage, exact replay after reopening and expiry, changed request bytes rejection,
and replacement CDK operation rejection. Its final transition is modeled, not
captured from a live CDK transport POST; that integration is still pending.
Wallet format and whitespace checks also pass. No submission permission is added.
Fresh locked/offline wallet strict Clippy passed for all targets/features (9.22s).

Submission-claim checkpoint: full wallet all-features suite passed (38 library
and three binary tests, zero failures/ignored). The real-acquisition fixture now
also verifies expired first-claim rejection, exact mint URL/request matching,
one successful durable claim, immediate duplicate refusal, and duplicate refusal
after reopening the journal. No HTTP submission occurs. Formatting and whitespace
checks pass; transport integration and post-submission reconciliation remain open.
Fresh all-targets/all-features wallet strict Clippy passed (10.69s).

Melt-input cryptographic/state checking is added to the acquired-output fixture:
valid owned proofs/unspent states, pending/spent refusal, wrong mint keys, missing
DLEQ and forged signatures. Verification is pending: root Cashu test handle 91103
is still compiling dependencies; wallet suite handle 51643 is queued behind its
shared target-directory lock. Logs are retained at
`/tmp/lg-cashu-core-tests-current.log` and `/tmp/lg-cashu-melt-input-tests.log`.
No result is inferred from the queue. These checks are not yet wired into a melt
transport submission path.

The operation gate's treasury integration now has a real-CDK confirmation fixture
using `MeltConfirmOptions::skip_swap()`. The synthetic transport counts a melt
boundary call and returns a connection error; the fixture requires the durable
request and `submitted` claim to exist and a second attempted POST to be refused
without reaching the mint again. This new path is not yet verified: core test
handle 91103 is compiling the root library, and wallet handle 51643 remains queued
behind its target lock. No new test success is claimed for these source edits.

Initial melt-reply retention is added before CDK response decoding/cleanup, with
fixture assertions for refusal before submission, exact-repeat acceptance and
conflicting-response rejection. These edits remain unverified while root test
handle 91103 runs and wallet handle 51643 waits for the shared target lock.
Both handles were polled and confirmed live; neither was restarted. Formatting
and patch whitespace checks pass; no new suite result is claimed.

Core Cashu verification completed: the initial sandbox run passed nine tests and
failed six at local socket creation (`EPERM`). Re-running the same compiled test
binary with isolated temporary state and local sockets permitted passed all 15
Cashu tests, zero failed/ignored; retained log:
`/tmp/lg-cashu-core-tests-native.log`. This is the Cashu filter, not the full root
verification gate.

The wallet run exposed a real integration mismatch: CDK's `MeltRequest::new`
removes DLEQ from wire inputs. Verification now matches exact input secrets,
signatures, amounts, keysets and mint to stored wallet proofs and verifies their
DLEQ, while still requiring exact fresh unspent states. The old fixture process
was deliberately terminated after its known failure while it remained in CDK
recovery; this was not treated as an observation timeout. The corrected suite is
running as handle 96030 with log `/tmp/lg-cashu-melt-input-corrected-tests.log`.
Its lost-reply confirmation now uses a bounded cancellation window and must still
prove one mint call, durable submitted state and zero additional calls on retry.
Corrected wallet handle 96030 subsequently passed: 39 library + three binary
tests, zero failures/ignored, doc tests passed (zero cases). This includes the
actual CDK pre-POST capture, lost-reply/cancellation and duplicate submission
refusal. It does not prove paid melt/change reconciliation or external mint
qualification. Fresh strict wallet Clippy is running as handle 16638.
Strict wallet Clippy handle 16638 passed for all targets/features (10.41s).
Wallet formatting and whitespace checks pass on the corrected source.

Paid-quote validation checkpoint: full locked/offline wallet suite passed with
41 library + three binary tests, zero failures/ignored, doc tests passed (zero
cases). New signed-invoice fixtures cover original paid evidence after expiry,
optional echo omission, pending state, replacement quote/amount/reserve/expiry,
wrong invoice/unit, missing/malformed preimage and a valid-length wrong preimage.
This validates mint evidence only; owned-change recovery and settlement report
export remain open. Wallet formatting passes. Strict Clippy is running as
handle 48263 with `/tmp/lg-cashu-paid-quote-clippy.log` retained.
Strict wallet Clippy completed successfully for all targets/features (8.19s).

Owned-change verification checkpoint: full locked/offline wallet suite passed
with 42 library + three binary tests, zero failures/ignored; doc tests passed
(zero cases). The new cryptographic fixture returns two sats from a three-sat
maximum change allowance and verifies the original wallet derivation, signature,
DLEQ and fresh state. Wrong seed, missing DLEQ, altered proof/signature/amount,
wrong mint and spent state all fail. This does not yet exercise end-to-end paid
melt restoration, durable change attribution or settlement reporting. Wallet and
root format checks and patch whitespace checks pass. Strict wallet Clippy is
running as handle 18605 (`/tmp/lg-cashu-change-clippy.log`).
Strict wallet Clippy passed for all targets/features (11.42s).

Durable wallet-report checkpoint: the first all-features run passed 43 library
and three binary tests, zero failures/ignored. A synthetic submitted-state
fixture verifies omitted-change refusal, late SQL-trigger failure rolling back
the result, explicit zero change with a one-sat fee, deterministic report values,
and original report replay after reopening. It is not an end-to-end paid-mint
fixture. A subsequent explicit quote-reserve fee bound was added; the exact final
source is being rerun as handle 57738 with output retained at
`/tmp/lg-cashu-report-final-tests.log`. Earlier Clippy passed before the fixture
and final bound, so a fresh final Clippy gate remains required.
Final report source passed the repeated full wallet suite: 43 library + three
binary tests, zero failures/ignored, doc tests passed (zero cases). Vendor source,
patch inventory and locked SQLite/CDK binding verification also passed. Final
strict Clippy is running as handle 37026.
Final wallet strict Clippy passed for all targets/features (10.84s).

Recovery-context checkpoint: full locked/offline wallet suite passed with
43 library + three binary tests, zero failures/ignored; doc tests passed (zero
cases). The durable-report fixture loads the original submitted context, verifies
the constrained quote-status URL, then reloads the same operation after journal
reopen/reconciliation. This is read-only journal recovery, not scheduled mint
polling or missing-change restoration. Format and whitespace checks pass. Disk
remains at approximately 13 GB free with the shared compact build target.
Strict wallet Clippy passed for all targets/features (9.36s).

Original quote polling checkpoint: full locked/offline wallet suite passed with
44 library + three binary tests, zero failures/ignored; doc tests passed (zero
cases). A transport fixture permits only the exact original quote GET and panics
on POST, form, WebSocket or proxy use. It covers paid preimage validation,
unpaid/pending unresolved results, conflicting response terms, and a replacement
quote identity rejected before any transport call. Formatting/whitespace checks
pass. This is one read-only poll, not a complete recovery scheduler or external
mint qualification. Strict Clippy is running as handle 93123.
Strict wallet Clippy passed for all targets/features (12.99s).

NUT-09 restore normalization checkpoint: full locked/offline wallet suite passed
with 44 library + three binary tests, zero failures/ignored; doc tests passed
(zero cases). The change fixture now checks original-output normalization and
rejects duplicate, unrelated and non-prefix restored outputs. The asynchronous
restore adapter compiles but has not yet been exercised through a transport
fixture; no end-to-end restoration claim is made. Durable raw restoration evidence,
proof reconstruction and coordinator integration remain outstanding. Formatting
and whitespace checks pass. Strict Clippy is running as handle 75856.
Strict wallet Clippy passed for all targets/features (12.33s).

Durable restoration-evidence checkpoint: full locked/offline wallet suite passed
with 44 library + three binary tests, zero failures/ignored; doc tests passed
(zero cases). The report fixture now preserves the raw paid quote with omitted
change, records the explicit empty restore result twice, reopens the journal,
then exercises atomic rollback and report commit using the retained evidence.
No paid-response mutation is used. This covers durable zero-change restoration
accounting; nonzero restored-change end-to-end transport/reconstruction remains
unqualified. Formatting and whitespace checks pass. Strict Clippy is running as
handle 58609.
Strict wallet Clippy passed for all targets/features (9.57s).

Nonzero restore/reconstruction checkpoint: full locked/offline wallet suite passed
with 44 library + three binary tests, zero failures/ignored; doc tests passed
(zero cases). The cryptographic change fixture now exercises the asynchronous
NUT-09 adapter through a transport allowing only the exact restore URL and frozen
output list, then normalizes signatures, reconstructs two sats of change and
verifies ownership against supplied fresh states. Missing signature DLEQ fails.
This closes the direct adapter-test gap; it does not qualify the full journal/CDK
persistence/restart coordinator or a real mint. Formatting/whitespace checks pass.
Strict Clippy is running as handle 62705.
Strict wallet Clippy passed for all targets/features (10.21s).

Recovery-coordinator checkpoint: source now serializes original quote recovery,
restoration evidence, reconstruction, fresh states, CDK persistence and terminal
accounting under the shared writer lease. The durable-report fixture was converted
to use real CDK SQLite and the coordinator, checking busy-lease refusal before
network contact and completed-report replay without another GET. Its first run
found CDK rejects an empty proof-ID lookup (`Empty IN clause for placeholder: ys`)
for zero change. The coordinator now skips that lookup when no change proofs
exist. Corrected all-features suite is running as handle 57272 with log
`/tmp/lg-cashu-recovery-coordinator-final-tests.log`; no passing result is yet
claimed for this corrected coordinator fixture. Nonzero persistence/crash-window
qualification remains outstanding.
Corrected coordinator suite passed: 44 library + three binary tests, zero
failures/ignored; doc tests passed (zero cases). Format/whitespace checks pass.
Fresh final strict Clippy is running as handle 50790.
Final strict wallet Clippy passed for all targets/features (10.39s).

Nonzero recovery crash-window qualification: full locked/offline wallet suite
passed with 45 library + three binary tests, zero failures/ignored; doc tests
passed (zero cases). The new coordinator fixture starts from synthetic submitted
state with 103 input sats, 100 deposit, two sats verified change and one sat fee.
A late journal trigger aborts report commit after CDK SQLite stores the change.
The fixture verifies that one two-sat proof exists and no report committed, reopens
the journal and CDK store, then reconciles the same proof and replays the immutable
report. The only permitted network operations are original quote GETs and exact
change-state queries; completed replay adds no call. This qualifies that local
crash window, not an external mint/Strike/feed end-to-end run. Formatting and
whitespace checks pass. Strict Clippy is running as handle 38765.
Strict wallet Clippy passed for all targets/features (9.24s).

Recovered-change source attribution checkpoint: full locked/offline wallet suite
passed with 46 library + three binary tests, zero failures/ignored; doc tests
passed (zero cases). A new allocation fixture splits a two-sat change denomination
across two original acquisitions, preserving the exact frozen source totals,
and rejects missing/excess value. The zero/nonzero recovery fixtures check the
durable source-part totals alongside report accounting. This adds provenance,
not permission to reserve/spend recovered change; change-aware proof selection
and denomination splitting remain pending. Formatting/whitespace checks pass.
Strict Clippy is running as handle 75743.
Strict wallet Clippy passed for all targets/features (8.74s).

Change-aware reservation checkpoint: full locked/offline wallet suite passed with
46 library + three binary tests, zero failures/ignored; doc tests passed (zero
cases). The nonzero recovery fixture now seeds original acquisition/hold evidence,
reserves its verified two-sat change under a new bound invoice, accepts exact
selection replay after expiry, and rejects another intent exceeding remaining
custody. Source-value accounting subtracts completed change, while old proof holds
remain. Combined output/part views drive selection and prepared/final validation.
A dedicated shared-denomination reservation/partial-owner rollback fixture and
unreleased-schema migration checks remain outstanding. Formatting/whitespace
checks pass. Strict Clippy is running as handle 38887.
Strict wallet Clippy passed for all targets/features (8.92s).

Shared-proof reservation qualification: full locked/offline wallet suite passed
with 47 library + three binary tests, zero failures/ignored; doc tests passed
(zero cases). The new fixture seeds one two-sat recovered proof with one sat
attributed to each of two acquisitions. Selecting one source fails and rolls back
all proof holds; selecting both succeeds with one global hold, rejects a competing
intent and replays after reopening/expiry. A one-proof limit now counts unique
Ys rather than repeated source references. This closes the shared-denomination
reservation fixture gap; denomination splitting and migration qualification remain
outstanding. Formatting/whitespace checks pass. Strict Clippy is running as
handle 43131.
Strict wallet Clippy passed for all targets/features (8.85s).

Treasury quote-gate qualification (2026-11-16): the prior run failed one
fixture because it omitted durable quote binding without invoking the new gate.
The corrected fixture now creates its quote through CDK's actual `melt_quote`
API, with the gate committing an attempt and binding validated terms before CDK
persists its quote. It rejects unarmed and altered-unit requests before mint
contact and duplicate creation after success. Separate cases cover lost replies
and invalid returned amounts: the attempt/request and any reply remain durable,
no quote is bound, no melt is sent, and journal reopen/expiry cannot authorize a
replacement request. The existing real-CDK melt fixture still proves one outbound
submission before reply loss.

Full locked/offline wallet suite passed: 49 library + three binary tests, zero
failures/ignored; doc tests passed (zero cases). Wallet formatting, all-target/
all-feature strict Clippy, whitespace and exact vendor verification passed.
Logs: `/tmp/lg-cashu-quote-gate-tests.log` and
`/tmp/lg-cashu-quote-gate-clippy.log`. These are isolated protocol fixtures, not
real-provider or production qualification. Retained-quote adoption and complete
service orchestration remain unfinished; full root gates remain outstanding.

Retained quote recovery qualification (2026-11-16): quote creation and restart
adoption now share validation of the durable original reply. The attempt stores
its original endpoint; a changed mint URL cannot reinterpret the reply. A fixture
forces quote binding to fail after reply retention, drops the wallet/gate, reopens
both stores and recovers the same quote into real CDK SQLite without another
quote or melt request. It checks writer contention, first adoption after expiry,
local replay after persistence, and refusal to overwrite conflicting CDK terms.
Lost replies still return unresolved, invalid terms remain rejected, and prepared
work cannot be rewound by quote adoption. The source remains undeployed, and
wallet-schema migration/service scheduling still need qualification.
Final locked/offline wallet suite passed: 50 library + three binary tests,
zero failures/ignored; doc tests passed (zero cases). Strict all-target/all-feature
wallet Clippy passed (9.28s); wallet/root formatting and whitespace checks passed.
Logs: `/tmp/lg-cashu-quote-recovery-final-tests.log` and
`/tmp/lg-cashu-quote-recovery-clippy.log`. Full root and live-provider acceptance
remain outstanding.

Treasury executor qualification checkpoint (2026-11-16): an explicit executor
now constructs a wallet with the scoped gate, requests a quote for reserved
sources, records a single preparation claim, loads held proofs, binds CDK's
prepared operation and submits with `skip_swap`. New fixtures exercise a lost
submission reply and missing CDK outputs while retaining custody journal state.
Both reopen the wallet stores and require the same preparation claim to prevent
replacement work. The initial fixture tried to reacquire the process writer lock
immediately after dropping CDK and failed because a connector reference remained
alive. The corrected worker-restart fixture retains the process-level lock while
reopening stores; it is not a process-exit/lock-release qualification.
Corrected full locked/offline wallet suite passed: 52 library + three binary
tests, zero failures/ignored; doc tests passed (zero cases). Strict wallet Clippy
passed for all targets/features; root/wallet formatting and whitespace checks
passed. Logs: `/tmp/lg-cashu-executor-tests.log` and
`/tmp/lg-cashu-executor-clippy.log`. This does not qualify actual mint payment,
core settlement orchestration, process shutdown, preparation recovery, or the
remaining full-root gates.

Prepared treasury resume qualification (2026-11-16): the executor can now stop
after binding a preparation and later resume its original CDK operation. A new
fixture prepares without any melt call, reopens wallet stores, resumes the same
operation ID once, and rejects subsequent resume/fresh-submit attempts after
reply loss. It also temporarily assigns the CDK quote to another operation and
requires refusal before mint contact, then restores that fixture ownership.
The resume adapter uses the pinned public CDK confirmation entry point with
`skip_swap`; journal and CDK state must both prove a never-submitted preparation.
A frozen final request, advanced CDK state, missing proofs, changed ownership or
expired execution authority cannot be converted into a new attempt. This is a
worker/store restart fixture, not process-exit qualification or a real payment.
The first strict Clippy run found a test-module ordering issue; the context type
was moved before the test module. Final qualification is recorded below.
Final locked/offline wallet suite passed: 53 library + three binary tests,
zero failures/ignored; doc tests passed (zero cases). Strict wallet Clippy and
root/wallet formatting/whitespace checks passed. Logs:
`/tmp/lg-cashu-resume-final-tests.log` and `/tmp/lg-cashu-resume-clippy.log`.
Repository-wide all-target/all-feature strict Clippy has started as handle
44859, log `/tmp/lg-cashu-core-full-clippy.log`; no full-root result is claimed
at this checkpoint.

CI coverage checkpoint (2026-11-16): the main CI workflow now includes a separate
Cashu-wallet job for exact vendor verification, tampering fixtures, formatting,
strict all-target/all-feature Clippy and the locked wallet suite. The Security
workflow separately audits the wallet lockfile without inheriting the core RSA
exception. Both jobs use the repository's Rust 1.88 toolchain and run from the
isolated wallet workspace. Local YAML parsing and shell syntax checks pass;
GitHub execution of these new jobs has not yet occurred. Vendor tampering tests
passed (two tests, six mutation subcases).

Fresh advisory fetches loaded 1,509 advisories. Core audit found no vulnerabilities
and the reverse RSA tree is empty; a follow-up no-fetch core audit exited zero.
Wallet audit exited zero with the previously documented unmaintained `paste`
warning (RUSTSEC-2024-0436), not a clean-maintenance claim. Logs:
`/tmp/lg-cashu-core-audit.log`, `/tmp/lg-cashu-core-audit-confirmed.log`,
`/tmp/lg-cashu-core-rsa-tree.log`, `/tmp/lg-cashu-wallet-audit.log`.
Full core Clippy continues as handle 44859. The full locked/offline root test
suite is queued as handle 63558, log `/tmp/lg-cashu-core-full-tests.log`, using
private temporary fixture state and local sockets outside the sandbox. Neither
full-root result is claimed at this checkpoint.

Full core strict Clippy completed successfully for all targets/features in
5m19s (`/tmp/lg-cashu-core-full-clippy.log`). The full core test suite, handle
63558, has started compilation. CI now explicitly tests the shared receipt and
Nostr packages, whose unit tests are not run merely by testing their dependents.
The initial local command incorrectly supplied `--all-features` for packages
outside the root workspace; Cargo rejected that combination before compiling.
Neither shared package defines optional features. The corrected locked command
omits that flag and is queued as handle 67532 with log
`/tmp/lg-cashu-shared-contract-tests.log`. The CI shell/YAML syntax check passed;
shared/core test completion is still pending at this checkpoint.

Private intake checkpoint (2026-11-16): the shared receipt package now contains
a strict versioned receive-frame contract, and the wallet has a kernel-peer-first
single-connection handler. The fixture sends no frame bytes to a same-UID socket
and proves immediate rejection; its internal admission checks submit a validated
P2PK proof via ordinary Cashu and then a signed nutzap, obtaining one acquisition
identity with no mint request or exportable receipt. Wrong epochs and disabled
intake fail. A shared contract fixture rejects unknown fields, unsupported
versions, nil delivery IDs and oversized frames. Distinct-UID positive socket
qualification, listener provisioning, public forwarding and scheduling remain
unimplemented; these fixtures do not claim a running payment endpoint.

Final wallet suite passed: 54 library + three binary tests, zero failures/ignored;
shared receipt tests passed (three), shared Nostr tests/doc tests had zero cases.
Strict all-target/all-feature wallet Clippy and explicit all-target shared-package
Clippy passed. CI now includes explicit shared-package format/lint/test commands.
Logs: `/tmp/lg-cashu-ingress-tests.log`,
`/tmp/lg-cashu-ingress-contract-tests.log`,
`/tmp/lg-cashu-ingress-contract-clippy.log`, `/tmp/lg-cashu-ingress-clippy.log`.
The full core suite (handle 63558) is still running without reported failures.
Its compile began before this new shared intake module; its eventual result must
not be presented as final exact-source acceptance of the new private protocol.

Core gate result checkpoint: the full-root library suite completed with 464
passed, zero failed and six designated privileged fixtures ignored (429.81s).
Remaining binary/integration tests are still running under handle 63558. This
qualifies the compiled pre-ingress core baseline, not a final acceptance claim
for later shared protocol additions; the privileged matrix remains required.

Private listener implementation now reuses the canonical protected IPC module.
Its new fixture checks mode 0660, refusal to replace an active socket, continued
acceptance after two rejected peers, explicit shutdown, stale socket reopen and
preservation of an unexpected regular file. Tests are running under handle
51974 (`/tmp/lg-cashu-listener-tests.log`), with strict wallet Clippy queued as
5339 (`/tmp/lg-cashu-listener-clippy.log`). Exact vendor verification passes.
Listener qualification completed: locked/offline wallet suite passed with
55 library + three binary tests, zero failures/ignored, and zero doc-test cases.
The protected-listener fixture passed. Strict all-target/all-feature wallet
Clippy passed; root/wallet formatting, whitespace and vendor verification passed.
The full core run has progressed into integration suites and remains active as
handle 63558; no integration completion claim is made yet.

Full-root baseline completion (2026-11-16): handle 63558 exited zero. Across
24 reported suites, 607 tests passed, zero failed and seven were ignored; the
library component had 464 passes and six ignores. The ignored cases are the
root retirement/bootstrap/maintenance fixtures and the real NIP-46 fixture, which
still require their separate isolated runners. Complete log:
`/tmp/lg-cashu-core-full-tests.log`. This baseline compiled before the later
private intake/forwarding additions and is not final-source qualification.

Core forwarding checkpoint: `src/cashu/intake.rs` adds a scoped, bounded peer-
authenticated adapter without public routing or ledger effects. Fixtures cover
disabled rails, excessive body size and saturated concurrency before connection,
and a wrong-UID socket receiving no bytes. Shared reply validation rejects
wrong delivery IDs, nil acquisition IDs and unsupported versions. Strict full
core Clippy is running as 3291 (`/tmp/lg-cashu-forwarder-clippy.log`); focused
native forwarding tests are queued as 27049 (`/tmp/lg-cashu-forwarder-tests.log`),
and shared contract tests as 2971 (`/tmp/lg-cashu-forwarder-contract-tests.log`).
No passing result is claimed for those additions yet.

Forwarder qualification completed: the two native core forwarding tests passed
(disabled/oversized/busy admission and wrong wallet UID receiving no payload),
with zero failures. Four shared receipt tests passed, including delivery-reply
identity validation. The preceding full-target core Clippy run exited zero
(3m58s); that run predates the HTTP/config wiring below.

HTTP/config checkpoint: optional core Cashu configuration and the default-off
`/api/v1/cashu` route now use the bounded forwarder. A local HTTP fixture covers
o configuration, browser preflight, unsupported media type, query rejection,
malformed JSON, payload limits, method restrictions and redacted unavailable-
wallet responses. The receive example has a commented disabled section only.
Strict full core Clippy is running as 29653 (`/tmp/lg-cashu-http-clippy.log`);
all core Cashu library fixtures are queued as 60793
(`/tmp/lg-cashu-http-tests.log`). No HTTP test pass is claimed yet. Positive
distinct-UID end-to-end forwarding, WAF qualification and independent clients
remain outstanding; no live config/service was changed.

HTTP/config results supersede the pending checkpoint above: core Cashu library
fixtures passed (12 tests, no failures/ignores), and full-target strict core
Clippy passed. Wallet startup configuration passed the complete separate wallet
suite (57 library plus three binary tests, no failures/ignores) and strict Clippy.
Logs: `/tmp/lg-cashu-http-tests.log`, `/tmp/lg-cashu-http-clippy.log`,
`/tmp/lg-cashu-service-config-tests.log`, and
`/tmp/lg-cashu-service-config-clippy.log`.

Protected-key loading checkpoint: startup reads fixed-length existing private
files, validates the receiving public-key binding, and refuses missing keys,
wrong lengths, invalid scalars, broad permissions, hard links and symlinks.
Synthetic fixtures also verify preserved bytes and absence of automatic key
creation. Direct `zeroize = 1.9.1` reuses the already locked dependency version;
no vendor or dependency-version update was made. Strict wallet Clippy passed;
the first complete wallet run had 57 library passes and one fixture failure:
the shared identity helper requires Tokio, so the new synchronous fixture was
corrected to `#[tokio::test]`. The suite is rerunning as handle 33024 (strict
Clippy recheck: 67913), with output at
`/tmp/lg-cashu-service-keys-tests.log`. This is isolated implementation, not a
running wallet daemon or a production key-provisioning result.

Protected-key qualification completed: corrected fixture and complete wallet
suite passed (58 library and three binary tests; zero failures or ignores).
Strict all-target/all-feature wallet Clippy, wallet/root formatting checks,
whitespace checks and exact vendor verification passed. Handles 33024 and 67913
both exited zero. The source still requires daemon/scheduler wiring and broader
final integration qualification; these tests do not authorize live activation.

Runtime assembly checkpoint: added isolated provisioning/restart fixtures for
missing databases, second-writer rejection, reinitialization refusal, changed
instance/epoch/seed, successful same-identity reopen, missing CDK storage,
dangling sidecars, and an existing unregistered journal. No public listener or
mint is contacted by these fixtures. Strict wallet Clippy is handle 69712
(`/tmp/lg-cashu-runtime-clippy.log`); complete wallet tests are handle 32506
(`/tmp/lg-cashu-runtime-tests.log`). Results are pending. The initial `cargo
check` identified an ambiguous `AsRef` type in the seed hash; it was corrected
to an explicit byte slice before this verification run.

Runtime assembly qualification completed: all 60 library tests and three binary
tests passed, with zero failures or ignores. The complete wallet strict Clippy
run and root/wallet formatting plus whitespace checks passed. Handles 69712,
32506 and 48421 exited zero. This verifies isolated startup assembly and existing
wallet fixtures; daemon scheduling, paired-snapshot restore validation and
production activation remain unfinished. Root disk still has approximately
12 GB available after verification.

Acquisition scheduler checkpoint: fixtures cover durable attempt charging,
backoff, later work passing unresolved earlier work, budget exhaustion without
proof release, restart preserving attempts, separate new/recovery permissions,
original ID retention, and arithmetic overflow rollback. Startup exercises an
idle default-disabled worker. Execution config tests reject implicit permission,
missing budgets, excessive bounds and unknown authority fields. The complete
wallet suite is running as 97632 (`/tmp/lg-cashu-scheduler-tests.log`); final
strict Clippy is 94714 (`/tmp/lg-cashu-scheduler-clippy-final.log`). The first
Clippy pass completed before final test additions. No final pass is claimed yet.

Scheduler qualification completed: 62 library and three binary tests passed,
zero failures/ignores. Final all-target/all-feature wallet Clippy, root/wallet
formatting and whitespace checks passed. This establishes the isolated durable
scheduling behavior and existing wallet regressions, not live mint/scheduler or
production service qualification. Logs retain the exact reported test totals.

Daemon entry-point checkpoint: added explicit command parser/error-redaction
fixtures, an executable CLI smoke test, and a combined listener/worker shutdown
fixture. The combined fixture uses synthetic keys/private temporary databases,
rejects a same-UID peer, shuts down and reopens the registered state. Actual
cross-UID positive delivery and SIGTERM during a real CDK operation remain
separate outstanding qualification. Tokio's signal feature uses already locked
packages; no dependency version was changed. Strict wallet Clippy is handle
78428 (`/tmp/lg-cashu-daemon-clippy.log`); all wallet/CLI tests are handle 59883
(`/tmp/lg-cashu-daemon-tests.log`). Results remain pending.

Daemon entry-point qualification completed: 63 library tests, three existing
compatibility-binary tests, two daemon command tests and one executable CLI
integration test passed (69 total, zero failures/ignores). The combined service
shutdown/reopen fixture passed. Final strict all-target/all-feature wallet Clippy
passed as handle 96055, logged in `/tmp/lg-cashu-daemon-clippy-final.log`; test
handle 59883 exited zero. Formatting, whitespace and vendor integrity checks
also passed. No daemon was installed, no live keys were loaded, and no production
configuration or service changed. Actual signal-driven shutdown during financial
work and positive cross-UID delivery remain unqualified.

Receipt acknowledgment checkpoint: added shared mismatch/size/epoch contract
fixtures, wallet wrong-core-UID/no-bytes fixture, and canonical acknowledgment
persistence/restart/conflict coverage. The core result type moved to the shared
crate without changing its fields or serialized representation. Wallet Clippy
passed before the final journal fixture addition. Complete wallet tests: 22010
(`/tmp/lg-cashu-receipt-delivery-tests.log`); shared contract tests: 41809
(`/tmp/lg-cashu-ack-contract-tests.log`); core strict Clippy: 75002
(`/tmp/lg-cashu-ack-core-clippy.log`); core Cashu library tests: 32669
(`/tmp/lg-cashu-ack-core-tests.log`). These final results remain pending.

Receipt delivery fixture results: complete wallet/CLI suite passed (65 library,
three compatibility-binary, two daemon and one executable CLI test; 71 total,
zero failures/ignores). All five shared receipt/acknowledgment contract tests
passed. Handles 22010 and 41809 exited zero. Root/wallet/shared formatting and
whitespace checks passed. Core Clippy 75002 is compiling; core Cashu tests 32669
and final wallet Clippy 3596 are queued behind the shared build directory lock.
Final wallet Clippy log: `/tmp/lg-cashu-receipt-delivery-clippy-final.log`. These
three handles were confirmed still running; do not restart them merely because
the observation call yielded. Core exact-source acceptance remains pending.

Core receipt listener checkpoint: optional core configuration and task supervision
are wired. New tests cover rejection before reading a length from the wrong UID,
commit-derived acknowledgment replay after reopening with imports/budgets disabled,
and invalid budgets/duplicate policy/epoch rejection. The successful import fixture
uses the existing test-only authenticated-receipt helper; it is not proof of a
positive cross-UID socket exchange. Root Clippy for the prior shared acknowledgment
change (75002) exited zero. Prior core Cashu tests 32669 and final wallet Clippy
3596 are still active/queued. New exact-listener Clippy: 69275
(`/tmp/lg-cashu-receipt-server-clippy.log`); Cashu core tests: 82386
(`/tmp/lg-cashu-receipt-server-tests.log`). Results remain pending.

Prior acknowledgment integration checks completed: core Cashu tests 32669 passed
(12 tests, zero failures/ignores), and final wallet Clippy 3596 exited zero. These
precede the newest receipt-listener/outbox source qualifications.

Receipt outbox checkpoint: independent durable retry admission and default-off
configuration now connect acquired receipts to the authenticated delivery client.
Fixtures cover skipping unacquired work, passing a lost-ACK head, backoff and
attempt preservation across reopen, retaining original receipt bytes and exhausted
pending state, and overflow rollback. Existing runtime shutdown now includes the
outbox; disabled startup exercises its idle tick. Exact outbox strict Clippy is
44764 (`/tmp/lg-cashu-receipt-outbox-clippy.log`); complete wallet tests are 1676
(`/tmp/lg-cashu-receipt-outbox-tests.log`). Core receipt-listener Clippy 69275 is
compiling, with its Cashu tests 82386 queued. No final pass is claimed yet.

Core receipt-listener Clippy 69275 exited zero. Its core test handle 82386 is
still compiling; outbox Clippy/tests 44764/1676 remain queued behind the shared
target. The running compiler was verified on the host with CPU/RSS and elapsed
process observations; no build was restarted because polling yielded.

Private status checkpoint: added an operator command and read-only snapshot
reader, with a 65-operation fixture covering output bounds, held-work priority,
exhausted attempts, private-field omission, missing-state refusal, repeated-read
stability and reads while the writer lock is held and seed file is unavailable.
CLI integration now checks status configuration errors too. Exact status Clippy
is 75779 (`/tmp/lg-cashu-status-clippy.log`), wallet/CLI tests are 51298
(`/tmp/lg-cashu-status-tests.log`). Final results are pending.

Listener/outbox/status verification completed before adding the cross-UID helpers:
all 15 core Cashu tests passed, including the listener fixtures. Outbox/status
wallet suites passed with 68 library plus six binary/integration tests (74 total,
zero failures/ignores), and both strict wallet Clippy checks exited zero. Handles
82386, 44764, 1676, 75779 and 51298 are terminal. Formatting/whitespace passed.

Positive cross-UID qualification preparation now adds two explicit ignored native
child fixtures and `deploy/scripts/test-cashu-cross-uid.py`. The runner requires a
fresh network namespace, uses unused numeric non-root UIDs without creating users,
and copies trusted artifacts into a root-owned temporary fixture. Only scratch
state/ownership is changed. It is designed to exercise the production receipt
server/client, lost acknowledgment, duplicate and conflicting replay, wallet ack
persistence, and one seven-sat core credit/event. Acquisition evidence is synthetic;
this does not qualify a real mint. Helpers are not ordinary skipped acceptance:
the runner must actually execute both markers before this gate is accepted.

Exact native helper builds are running/queued as 28835 and 66097;
core artifact stream `/tmp/lg-cashu-cross-uid-core-build.jsonl`, wallet stream
`/tmp/lg-cashu-cross-uid-wallet-build.jsonl`. No cross-UID pass is claimed yet.

Cross-UID CI wiring: core and wallet jobs select exactly one native library-test
artifact from a successful fresh Cargo JSON stream, upload the two artifacts, and
a dependent job runs the isolated cross-UID fixture with both. Selection refuses
ambiguous/foreign/out-of-target binaries and incomplete/failed build streams. The
fixture result records executable SHA256 values. Native binary artifacts expire
after one day; result evidence after 14 days. The core CI job now uses the same
zero-debug-symbol/non-incremental build profile as local verification and the
wallet job, keeping native artifacts bounded without changing debug assertions.
Python AST and workflow dependency/YAML checks passed; neither the local actual
UID exchange nor a GitHub Actions run is claimed yet.

Actual cross-UID receipt exchange passed on 2026-11-16. The root runner operated
only in a fresh network namespace and temporary fixture tree; core UID 60000 and
wallet UID 60001 were selected as unused numeric identities without creating
accounts. Production receipt server/client code accepted the synthetic acquired
receipt, retained a dropped acknowledgment, acknowledged exact retries, and
rejected the conflicting receipt. The wallet persisted its original canonical
acknowledgment; the core verified exactly seven sats and one payment event.
Both explicit ignored child fixtures actually executed and emitted their required
markers. The runner exited zero (87916); native helper builds 28835/66097 also
exited zero. Evidence with executable hashes: `cashu-cross-uid-20261116.json`.

This closes the positive kernel-UID receipt exchange fixture gap. It does not
qualify mint acquisition, HTTP-to-wallet intake under separate UIDs, production
provisioning, coordinated rollback/restore, or a GitHub Actions run. No real funds,
installed keys, services or physical feeder were involved. Final helper-source
Clippy is running as core 22666 and wallet 37981; wallet suite 33024 is queued.
The native core Cashu regression run is 50077. Logs use the
`/tmp/lg-cashu-cross-uid-{core,wallet}-*` prefix. Results remain pending where noted.

Final cross-UID helper checks completed: core Clippy 22666, wallet Clippy 37981
and wallet suite 33024 exited zero. The latter reports 68 library plus six
binary/integration passes, one explicitly ignored cross-UID child which was
separately executed successfully by the real-UID runner. No GitHub Actions result
is implied by local workflow wiring.

NIP-61 relay-reader checkpoint: bounded WSS/public-DNS adapter and exact
recipient/mint/kind/window filters added without runtime activation. In-memory
WebSocket fixtures exercise REQ shape, signed-event selection, forged signatures,
wrong mint/subscription, duplicates, ordered results, saturation, binary frames
and CLOSED without false coverage. Initial strict wallet Clippy passed and the
complete suite passed (71 library plus six binary/integration tests, one separately
qualified ignored child). Vendor integrity passed. A final change moves encoded
query-size rejection before DNS/TLS; final Clippy 84107 and wallet tests 56910
are running, logs `/tmp/lg-cashu-relay-final-clippy.log` and
`/tmp/lg-cashu-relay-final-tests.log`. Format checks are 33081. Positive live TLS
relay behavior and durable backfill/admission are not yet qualified.

Final relay-reader checks completed: strict all-target/all-feature wallet Clippy
84107 and complete wallet/CLI tests 56910 exited zero. The suite reports 71
library plus six binary/integration passes (77 total), zero failures and the one
explicit cross-UID child already qualified by its separate runner. Formatting,
whitespace and exact vendor checks passed. This includes encoded query rejection
before DNS/TLS. Durable relay/backfill state and runtime intake remain unfinished;
no real relay or mint was contacted by these fixtures.

### Durable relay discovery checkpoint implementation — 2026-11-16

The wallet journal now owns bounded relay scan registrations, with independent
recent and historical lanes. Each registration binds an immutable scope digest,
relay, historical floor, window size and overlap. Policy/key inventory changes
must use a new scope; historical acquisition records remain intact. Registrations
are capped at 128 rows, and individual windows at one day. No relay event creates
credit through this checkpoint API.

A persisted 30-second claim precedes the bounded reader operation. A restart
replays interrupted windows after the lease expires, while a superseding UUID
fences late results. A historical cursor advances only after the caller has
persisted all event dispositions and observed an uncapped EOSE. Incomplete replies
leave it unchanged. Saturated replies narrow the same range, retaining a
one-second saturated range as unresolved. Recent rolling samples have their own
lease and do not claim historical completeness. Completed historical passes return
to the configured floor, allowing later passes to discover delayed older events.

This is a storage primitive, not an activated relay worker. Scope derivation from
complete protected configuration, runtime scheduling/admission, bounded rejection
diagnostics, and signer/history publication remain outstanding. The existing
reader's signed-event validation and the common proof-Y admission index remain
separate authorities. A crash after admission but before checkpoint completion
must replay through that same proof index. No production state is opened by the
fixture tests.

Checkpoint validation: three new journal tests cover restart/lease retention,
stale completion and duplicate completion rejection, overlapping progress,
completion clock rollback/overflow, repeated historical passes, scope immutability,
registration capacity, saturation refinement, and independent recent/relay lanes.
Final-source wallet formatting and strict all-target/all-feature Clippy pass;
the full offline wallet suite passes with 80 tests successful, zero failures and
one ignored cross-UID helper (separately exercised by the documented two-UID
fixture). Vendor integrity verification and repository formatting/whitespace
checks also pass. Logs: `/tmp/lg-cashu-relay-scans-final-clippy.log` and
`/tmp/lg-cashu-relay-scans-final-tests.log`. These results qualify this source
increment only, not the unfinished full implementation or production activation.

### Default-off relay runtime integration — 2026-11-16

The development daemon now supervises a serialized relay worker alongside private
intake, acquisition and receipt delivery. Optional `relay_intake` configuration
is strictly bounded and defaults off; enabled discovery also requires enabled
nutzap admission with qualified mint policies and pinned keys. Offline config
validation checks the complete wire filter before state/key loading. No network
request occurs during worker construction or when disabled.

The discovery scope hashes the wallet instance/epoch, recipient, receiving public
key, complete sorted mint/keyset inventory, validation limits and scan policy.
The worker rotates through both lanes of every configured relay, including after
network failure. Returned events use the same signature/DLEQ/P2PK/proof validation
and common proof-Y admission journal as private intake. Invalid events and typed
proof/source conflicts are rejected without blocking later valid events. Storage
and internal-consistency failures are not treated as public rejection: they leave
the checkpoint unresolved and fail the supervised runtime. Completed scan counters
are committed with its checkpoint in a bounded companion table; no unbounded
public rejection log or token body is added.

The worker bounds each admission pass to five seconds between events, with the
reader's existing 20-second network deadline and byte/event caps. If processing
is incomplete, historical progress is retained for replay. Cancellation preserves
claims and any already admitted proofs. Counts include duplicate admissions, not
new money or acquired funds. Private status exposes at most 128 retained lane
records and the last completed disposition counts, omitting relay query strings.

New fixtures cover invalid input followed by valid proofs, partial proof overlap
followed by independent valid input, replay through another relay after wallet
reopen, default-off no-query behavior, network-failure lane fairness, storage
failure without checkpoint advancement, policy/key scope changes, configuration
bounds, and bounded private status. Relay transport remains separately covered by
the in-memory signed websocket fixture; no test here contacts a live relay or
uses real funds. Historical-key recovery, production TLS interoperability,
10019/7376 publication, treasury scheduling and full deployment remain unfinished.

The NIP-61 reference for this relay/receipt work is now pinned to
[`nostr-protocol/nips` commit `999f9bfbf5fe00d5c2711fd24badb4e56748c9bc`, `61.md`](https://github.com/nostr-protocol/nips/blob/999f9bfbf5fe00d5c2711fd24badb4e56748c9bc/61.md).
The GitHub file-history API identified that revision as the latest change to
`61.md` when checked on 2026-11-16, and its immutable file was fetched and reviewed.
It specifies the separate receiving key, recipient/mint relay filtering,
mandatory DLEQ, and encrypted redemption-history content with sender read-relay
routing. The latter publication/signing work remains outstanding; pinning the
reference is not client interoperability evidence.

Final runtime-integration verification: strict all-target/all-feature wallet
Clippy passes, and the full locked/offline wallet suite reports **84 passed,
zero failures, one ignored cross-UID child helper** (the dedicated two-UID runner
was qualified separately as recorded above). All four new relay-runtime tests
pass, including reopened-wallet replay and query-redacted private status. The
checkpoint regression also verifies the current recent-window horizon after an
incomplete reply. Repository/wallet formatting, whitespace and exact vendor
integrity checks pass. Logs: `/tmp/lg-cashu-relay-runtime-complete-clippy.log` and
`/tmp/lg-cashu-relay-runtime-complete-tests.log`. No live relay, mint, payment,
signer publication or feeder was exercised by this qualification.

### NIP-61 publication intent staging — 2026-11-16

The wallet now constructs a bounded kind-10019 intent from validated active
nutzap configuration: qualified configured mints, explicit `sat` units, configured
public receive relays, and the dedicated receiving key represented as an x-only
key. Library-backed tests reconstruct the even-parity P2PK key; the project Nostr
identity cannot substitute for the receiving key. Announcement construction does
not sign, publish or enable any service. Query-bearing relay URLs cannot enter
this public announcement.

A separate durable publication table stages history only for a journal acquisition
that is already acquired/export-pending/credited and originated as a validated
nutzap. Its original event ID, sender and net acquired amount come from the wallet
journal, not caller-provided receipt claims. Ordinary Cashu and unacquired input
cannot create redemption history. A retry retains the first timestamp and exact
source/owner/amount binding. History preparation does not advance receipt delivery.

History plaintext contains only incoming direction, net acquired amount and sat
unit. No 7375 token event exists in this architecture, so no synthetic created-token
reference is generated. Public history tags contain the original redeemed nutzap
and sender. An encrypted envelope must be frozen before a kind-7376 unsigned event
can be produced, then native signature verification binds every field before the
exact original signed bytes are retained. Subsequent changed ciphertext, event
fields or signed bytes are rejected. Pending publication capacity is 1,024;
exhaustion does not mutate an acquisition or prevent its receipt export.

The bounded base64/version check is envelope validation only, not NIP-44 MAC or
decryption verification. Tests use explicitly synthetic envelopes and synthetic
Schnorr keys; they do not claim real NIP-44 interoperability. The purpose-restricted
signer must perform authenticated encrypt-to-self and durable response retention.
That signer RPC, scheduling, NIP-65 sender read-relay routing, relay acknowledgment
tracking, and runtime/CLI activation are still unfinished. Nothing in this increment
widens the existing core/NIP-46 signing allowlist or publishes an event.

Protocol snapshots used for this stage:
[NIP-61](https://github.com/nostr-protocol/nips/blob/999f9bfbf5fe00d5c2711fd24badb4e56748c9bc/61.md),
[NIP-60 history semantics](https://github.com/nostr-protocol/nips/blob/999f9bfbf5fe00d5c2711fd24badb4e56748c9bc/60.md),
and [NIP-44 envelope bounds](https://github.com/nostr-protocol/nips/blob/999f9bfbf5fe00d5c2711fd24badb4e56748c9bc/44.md).
The direct base64 dependency is pinned at 0.22.1, reusing the already locked
transitive package; no dependency package version changed.

Publication-stage verification: final-source strict all-target/all-feature wallet
Clippy passes. The full locked/offline wallet suite reports **89 passed, zero
failures, one ignored cross-UID helper** (separately qualified by the dedicated
runner recorded above). Five new tests cover configured announcement policy,
x-only key encoding, exact signed fields, private history/envelope gating,
acquisition-only history with reopen/retry, and backlog isolation. Formatting,
whitespace and exact vendor/lock binding checks pass. The cached advisory scan
(`cargo audit --no-fetch --no-yanked`) checks 331 dependencies against 1,509
advisories with no vulnerabilities and the existing allowed unmaintained
`paste` warning; it does not refresh advisories or query yanked packages.
Logs: `/tmp/lg-cashu-publication-qualified-clippy.log`,
`/tmp/lg-cashu-publication-qualified-tests.log`, and
`/tmp/lg-cashu-publication-audit.log`.

The installed `nak` helper reports v0.20.6; its encryption command help advertises
NIP-44 and the bunker credential form. This interface inspection is not a remote
encryption/signing qualification. The existing purpose-limited signer still lacks
Cashu RPC handling and its signing allowlist remains unchanged.

### Purpose-limited Cashu signing broker — 2026-11-16

The wallet workspace now contains a separate signing-broker implementation with
its own private SQLite journal and process lock. Explicit initialization refuses
existing database/sidecar state; normal open requires the retained broker/wallet
instance, epoch, wallet UID and project-owner binding. The broker must run under
a non-root UID different from the wallet caller. This source is not yet wired to
a daemon executable or an actual NIP-46 backend.

The policy separately defaults announcements and redemption signing off. Enabled
purposes require explicit new-request and retry budgets; history also has a net
amount bound. Announcements must match the configured receiving key, mint set and
receive-relay set exactly. New requests are bounded per sliding hour, pending
requests are capped at 1,024, future timestamps and clock rollback are rejected,
and attempts/cooldown are persisted before backend contact. Both acquisition ID
and original nutzap event have uniqueness guards against replacement-operation
replays. Request identity and its entire intent are immutable.

The broker retains the first structurally valid encryption response before
requesting decrypt-to-self. A failed/lost decrypt response therefore retries the
same ciphertext, and signing is prohibited until the decrypted plaintext matches
the intent. A lost signing response retries the same timestamp/tags/ciphertext;
a verified signature is retained as exact bytes before returning it. A cached
result needs no further backend call when it still matches current policy,
including when new signing is disabled. No arbitrary recipient, plaintext,
external ciphertext or general event kind is available in the request protocol.

Bounded Unix IPC authenticates the wallet UID before reading a request frame and
the signer UID before sending any request bytes. There is one request per
connection, bounded frame/operation/reply time, a serialized accept loop and a
fixed cooldown. The wallet creates signing requests from its registered instance,
epoch and immutable publication intent. It verifies the broker result and retains
its ciphertext/signed event together in one SQLite transaction. Failed retention
or acknowledgment loss leaves financial acquisition/receipt state unchanged.

Fixtures use a synthetic encrypt/decrypt backend and native synthetic Schnorr
signatures. They cover failed decrypt/sign responses across broker reopen,
immutable ciphertext/result replay, public-field alteration, source replacement,
attempt/hour limits, disabled/future-policy refusal, wrong-UID rejection before
payload I/O, and atomic wallet response retention. These are NOT real NIP-44 or
NIP-46 interoperability tests. A positive distinct-UID broker acceptance fixture,
actual bounded nak/bunker adapter, protected deployment configuration/CLI,
publication scheduling and sender read-relay delivery remain outstanding.

Broker-stage final verification: strict all-target/all-feature wallet Clippy and
the full locked/offline wallet suite pass: **95 passed, zero failures, one ignored
cross-UID child helper** (the older receipt fixture is qualified separately).
The six added tests exercise broker recovery/policy/IPC rejection and atomic
wallet response retention. Repository/wallet formatting, whitespace, and exact
vendor/lock verification pass. Logs: `/tmp/lg-cashu-broker-qualified-clippy.log`
and `/tmp/lg-cashu-broker-qualified-tests.log`. No live credentials, bunker,
relay publication, mint, transfer or feeder was used.

## Real Cashu signing adapter qualification — 2026-11-16

The broker now has a bounded nak subprocess adapter with fixed commands, private
stdin, cleared environment, pinned binary/client identity, protected production
paths, serialized operations, output limits, deadlines and child cleanup. Native
signature and exact-template checks still precede durable result acceptance.
The adapter does not supply the project's Nostr private key.

The initial real fixture exposed an incompatibility: stock nak v0.20.6 encrypt
and decrypt consume positional arguments, not stdin. The separate
[`cashu-nak-stdin` helper](../../tools/cashu-nak-stdin/README.md) adds a pinned,
bounded stdin patch without changing cryptography or replacing the installed
helper. Its build records 22 existing upstream main-package vet diagnostics as
an exact baseline; this is not a clean upstream vet claim. The patch's bounded
input tests pass.

[`cashu-nak-20261116.json`](cashu-nak-20261116.json) records the successfully
tested native wallet, patched client and stock bunker binary digests. The fixture
runs as unused UID 60000 in a fresh network namespace containing only loopback,
using public synthetic keys and temporary state. It verifies actual NIP-46
encrypt/decrypt/sign operations, NIP-44 plaintext recovery and tampered-MAC
rejection, exact kinds 10019 and 7376, and replay of a retained signed result
after the bunker stops. No external network, live credentials, funds, feeder or
installed service is used. Reproduce with `deploy/scripts/test-cashu-nak.py`
and an explicitly selected current native wallet test artifact.

The current full wallet crate suite passes 98 tests (92 library, three descriptor
CLI, two daemon unit and one daemon integration) with two isolated fixtures
ignored by default; the real bunker fixture above was run separately. Strict
all-target/all-feature wallet Clippy, formatting and both exact SQLite vendor
checks pass. Logs are `/tmp/lg-cashu-nak-current-full-tests.log` and
`/tmp/lg-cashu-nak-current-clippy.log`; the successful isolated fixture's machine
record is linked above.
Standalone broker startup/credential loading, positive distinct-UID broker IPC,
publication scheduling, sender read-relay delivery and production permissions
remain unqualified. This checkpoint supersedes the earlier synthetic-backend-only
limitation; it does not complete Cashu deployment acceptance.

## Standalone signer startup — 2026-11-16

`lightning-goats-cashu-signerd` now supplies protected offline configuration
validation, explicit journal initialization and existing-state serving with
SIGTERM/SIGINT shutdown. Registration binds the runtime to a separate non-root
UID; project identity, dedicated receiving key and NIP-46 client identity must
all differ. Socket, broker state and helper state are separate paths. Unknown
configuration fields and enabled signing without explicit budgets are rejected.

Only serving loads the fixed systemd `cashu-nostr-client` credential. Reads are
bounded, use no-follow/nonblocking descriptors and require a private regular
single-link file under canonical protected directory ancestry. Neither offline
checking nor initialization needs a credential or invokes a helper. No missing
journal is automatically initialized and no key is generated. Operator-facing
errors do not echo secret values or paths. The development runbook documents
provisioning requirements; no unit or credentials were installed.

New tests cover default-off policy and conflicting identities/paths, explicit
initialization and preserved existing bytes on retry/wrong UID, credential bounds,
permissions and symlink/hardlink rejection, and actual executable usage/redacted
configuration failures. This does not yet prove positive cross-UID installed
startup or a systemd credential-mount exchange; those remain explicit acceptance
work alongside publication scheduling and delivery.

Final startup-source suite: **102 passed, zero failed, two ignored** (95 library,
three descriptor CLI, two wallet daemon unit, one wallet daemon executable and
one signer executable test). The ignored real-bunker/receipt fixtures have their
separate earlier evidence; they were not rerun for this startup-only change.
The new private-directory test initially exposed a fixture with insufficient
permissions; using the existing explicit-0700 fixture helper preserves the
production rejection. Logs: `/tmp/lg-cashu-signer-cli-qualified-tests.log`.
Strict all-target/all-feature Clippy also passes on that final source
(`/tmp/lg-cashu-signer-cli-qualified-clippy.log`), as do repository/wallet
formatting, whitespace and both exact vendor checks. Full root application
acceptance and deployment gates remain outstanding for the broader Cashu change.

## Separate-UID signing exchange — 2026-11-16

[`cashu-signer-cross-uid-20261116.json`](cashu-signer-cross-uid-20261116.json)
records an actual kernel-authenticated Unix-socket exchange under unused signer
UID 60000 and wallet UID 60001. UID 60002 has socket-directory group traversal
but is rejected before payload I/O. The runner creates only temporary files and
numeric process identities inside a fresh loopback-only network namespace; it
does not create accounts or services or load production credentials.

The wallet starts with a synthetic acquired nutzap, prepares redemption history
and an announcement through its real journal APIs, loses the first signing
reply, and retries using the production broker client. The broker retains exactly
two operations, two attempts and two signed results. Both processes then stop
and reopen their journals. The wallet receives the identical retained bytes,
while a backend that refuses all new crypto operations records zero calls.
Conflicting replay is rejected, and the wallet's acquired financial state remains
unchanged. This exercises both 7376 and 10019 across separate UIDs, without
claiming that the synthetic encryption fixture verifies NIP-44 cryptography.
The earlier real-bunker qualification provides separate evidence for that path.

Reproduce using `deploy/scripts/test-cashu-signer-cross-uid.py --wallet-test
<selected-current-native-library-test>` under `sudo unshare --net --`. Native
artifact selection must use fresh successful Cargo JSON and
`select-cashu-test-artifact.py`, not an arbitrary glob. CI now invokes this runner
from the same previously built wallet artifact as the receipt cross-UID job;
that new CI step has not yet run remotely. Installed standalone startup with
systemd credential mounts and actual bunker permissions remains unqualified.

Final wallet regression suite: **102 passed, zero failed, four ignored**. The two
new ignored child fixtures were successfully invoked in both fresh/restarted
phases by the isolated runner above. Strict wallet all-target/all-feature Clippy,
formatting, whitespace, runner Python syntax and exact wallet vendor checks pass.
Logs: `/tmp/lg-cashu-signer-crossuid-tests.log` and
`/tmp/lg-cashu-signer-crossuid-clippy.log`. Full root application and production
acceptance gates remain outstanding for the overall Cashu implementation.

## Wallet publication signing scheduler — 2026-11-16

The wallet runtime now supervises a separate default-off signing worker.
Announcement/history permissions and attempt limits are explicit protected
configuration, independent of acquisition and core receipt delivery. Enabled
history signing stages one acquired nutzap per tick, preserving its source/net
amount and first creation time; ordinary and unacquired receipts are excluded.
The existing 1,024 pending-publication cap pauses staging without changing
financial state or preventing credit export.

A separate signing schedule charges an attempt before IPC and retains backoff
across restart/cancellation. The initial 180-second delay exceeds the client's
165-second deadline; exponential backoff caps at one hour. Later due records can
proceed while failed work waits. Exhaustion never substitutes a new operation;
an explicitly raised limit resumes the original intent. Accepted responses
retain ciphertext and exact verified signed bytes atomically. Optional signing
failures do not terminate the financial workers. Private status distinguishes
unsigned, signed-unpublished and retry-exhausted records.

New regression cases cover unavailable-signature fairness and eventual retry,
restart-preserved backoff, disabled no-I/O behavior, acquisition-only staging,
unchanged acquired/export state, attempt exhaustion, retry-transaction rollback,
clock overflow and backlog saturation. No relay publisher or automatic receiving
announcement is added by this stage. Sender read-relay routing, per-relay ACKs,
explicit operator announcement preparation and installed service acceptance remain
part of the unfinished implementation.

Final wallet suite: **106 passed, zero failed, four ignored**. Strict
all-target/all-feature Clippy, repository/wallet formatting, whitespace and both
exact vendor checks pass. Logs: `/tmp/lg-cashu-publication-worker-tests.log` and
`/tmp/lg-cashu-publication-worker-qualified-clippy.log`. Separate-UID and real
bunker fixtures retain their earlier evidence and were not rerun for the new
scheduler. Broader root application and production acceptance remain incomplete.

## Relay delivery primitives and shared NIP-65 rules — 2026-11-16

The core `RelayList` parser now delegates authority, expiry, direction, sorting
and eight-relay limits to `crates/nostr-event::relay_list`; its destination
validator and write-only-broadcaster exclusion remain unchanged. The Cashu
adapter uses that same parser with public WSS validation and excludes query-bearing
URLs and write-only broadcasters from sender read routes. This shares the
existing routing rules rather than creating a competing interpretation of NIP-65.

The wallet metadata query requires native event verification, exact author/kind,
matching subscription EOSE, bounded frames/bytes and nonsaturated results. It
queries at most eight configured discovery relays concurrently and selects the
newest valid visible list, breaking equal-time ties by lowest event ID. No
complete response yields a retryable error; a completed query without a valid
list explicitly returns no discovered routes. Sender hints cannot introduce
arbitrary discovery destinations. The existing checked DNS/TCP/TLS transport is
shared with nutzap intake, with a separate 12-second connection bound inside the
20-second overall operation deadline.

Publication sends the retained raw signed event unchanged inside the EVENT
frame. Only a positive, correctly shaped OK for that exact event ID succeeds.
Negative/wrong-ID acknowledgments, premature close, deadlines and bounded-input
exhaustion do not count as success. No relay result asserts downstream visibility.

The private journal atomically freezes up to 16 configured base destinations plus
up to eight sender read relays, retaining the signed sender-list evidence. Proposed
read destinations must exactly match the verified list for the redemption sender;
announcements cannot adopt a sender list. Each relay has its own durable attempt,
backoff and acknowledgment. Partial success survives restart, lost replies resend
identical bytes, exhausted attempts retain their original operation, and all
frozen destinations must positively acknowledge before `published` becomes true.
Route-plan insertion failures roll back the entire destination set. Publication
never mutates acquisition amounts, state or core receipt evidence.

Fixtures cover forged/expired/future/wrong-author metadata, direction and unsafe
routes, EOSE correlation/saturation, exact wire bytes, matching positive and
negative ACKs, timeouts, partial acknowledgment, restart and lost-reply replay,
route transaction rollback, forged destination evidence, and continued credit
export after history routing. These primitives are not yet connected to the
runtime delivery worker. Durable routing retries, independent enabled flags and
operator announcement preparation remain outstanding before any publication can
be enabled. No live relay publication occurred.

Current checkpoint: the full wallet suite passes **112 tests, zero failures,
four ignored**, including six new relay/journal cases. Core strict all-target/
all-feature Clippy passes. Both vendor checks, repository/wallet formatting and
whitespace checks pass. The fresh core audit passes against 1,509 advisories and
317 dependencies; RSA is absent from the active graph. The separate wallet audit
passes for 331 dependencies with the existing allowed unmaintained `paste 1.0.15`
warning (RUSTSEC-2024-0436). Logs are `/tmp/lg-cashu-relay-delivery-tests.log`,
`/tmp/lg-cashu-relay-core-clippy.log`, `/tmp/lg-cashu-relay-core-audit.log` and
`/tmp/lg-cashu-relay-wallet-audit.log`.

The full core suite subsequently passed in `/tmp/lg-cashu-relay-core-tests.log`:
**613 passed, zero failures, eight ignored**, including 470 library tests and
all existing relay-routing cases. No core source changed during the subsequent
wallet delivery-worker wiring. Final wallet validation for that wiring is
recorded in the following checkpoint.

## Default-off relay delivery runtime — 2026-11-16

The wallet now supervises route planning and delivery as an optional worker.
Announcement/history switches are independently false by default. Enabling a
purpose requires explicit safe base destinations and attempt limits; history
also requires bounded configured discovery relays. URLs are canonicalized for
uniqueness, query-bearing targets are rejected, and write-only broadcasters
cannot serve as metadata discovery sources.

Routing attempts have a durable schedule separate from signing and per-relay
publication. Missing metadata backs off and yields to other events. Each tick
can still deliver an already-frozen target after a routing failure. Both network
operations have 25-second worker deadlines over the transport's 20-second bound;
delivery's claimed timestamp accounts for time spent discovering metadata.
Backoff starts at 30 seconds and caps at one hour. Attempts are recorded before
network I/O, and no journal lock is held during network waits.

Both routing and delivery honor per-purpose switches, including already-routed
work. Pausing preserves destinations, attempts and acknowledgments. Old signed
owner identities cannot silently publish under a different runtime owner.
Optional errors do not terminate financial workers. Private status now includes
pending/exhausted route and relay-delivery counts without exposing sender lists,
raw event contents or credentials. No public publication setting was enabled.

The new end-to-end worker fixture starts with acquired nutzap history and a
prepared announcement, loses metadata and a relay reply, restarts the wallet,
and verifies fairness, exact-byte retry, one successful sender-read delivery and
unchanged financial state. Further fixtures cover paused-purpose retention,
default-off no-network behavior, URL aliases/unsafe targets, routing transaction
rollback and restart/exhaustion/clock-overflow behavior. The previously pending
full core suite passed: 613 tests, zero failures, eight ignored. The new wallet
suite passes **116 tests, zero failures, four ignored** in
`/tmp/lg-cashu-delivery-runtime-tests.log`. Installed credential-mount startup,
operator announcement preparation, live relay acceptance and remaining Cashu
milestone gates are still outstanding.
Final strict wallet all-target/all-feature Clippy also passes
(`/tmp/lg-cashu-delivery-runtime-final-clippy.log`), along with formatting,
whitespace and both exact vendor checks. Dependency versions are unchanged from
the just-completed core/wallet audits. The core source graph is unchanged from
its passing full test and strict Clippy runs.

## Offline announcement preparation and current-policy checks — 2026-11-16

`lightning-goats-cashud prepare-announcement <config> <operation-uuid>` opens
existing registered state and provisioned keys under the runtime UID/writer lock,
retains a configured kind-10019 intent, and prints its public metadata for review.
It starts no worker and performs no external operation. Retries of the explicit
UUID retain the first timestamp, signature and completed delivery state. Reusing
that UUID with changed advertised metadata fails without rewriting the prior
record. A new UUID requires a later timestamp than retained announcements, so
same-second replacements and clock rollback cannot produce ambiguous ordering.

Preparation checks qualified configured mints, pinned keys, dedicated receiving
key and enabled nutzap/relay intake. Announcement workers additionally require
receiving to be configured and compare each retained intent with the active
runtime's advertised policy before signing or publishing. Already-signed/routed
stale announcements are refused before external I/O, retain backoff and do not
block later work. Acquired-history delivery remains independent of whether new
receiving is enabled. No project Nostr credential is required to prepare metadata.

Fixtures verify offline initialization/opening, exclusive writer ownership,
retained operation/time/signature/ACK identity across restart, changed-policy and
clock rejection, zero signer/network calls for stale announcements, later-work
fairness and actual executable argument/error redaction. The full wallet suite
passes **119 tests, zero failures, four ignored** in
`/tmp/lg-cashu-announcement-cli-tests.log`. These are source/temporary-state tests,
not installed-command or live public receiving acceptance. Treasury orchestration,
coordinated backups, overlay presentation and remaining milestone gates remain
unfinished.

The final announcement strict all-target/all-feature Clippy also passed
(`/tmp/lg-cashu-announcement-final-source-clippy.log`). A subsequent positive
replacement assertion passed in `/tmp/lg-cashu-announcement-replacement-test.log`:
a later new UUID accepts changed advertised metadata while retaining the old
signed announcement. Root/wallet formatting and both vendor checks pass.

## Treasury preparation proof ownership — 2026-11-16

Preparation and resume now share an exact proof-response check before invoking
CDK: both expected and returned IDs must be unique, their sets must match,
each actual proof secret must derive its stored ID, and mint/unit/state/operation
ownership must match the requested stage. An unspent label does not override
an existing operation owner. Refusal preserves the original preparation claim.

A synthetic acquired-output integration fixture injects conflicting CDK ownership
and verifies zero melt attempts, one retained preparation claim, and refusal
again after reopening the stores. Existing positive preparation/resume fixtures
still pass. Additional cases reject duplicate/missing/substituted rows, cached
ID/secret disagreement and wrong operation ownership while accepting reordered
exact sets. Full locked/offline wallet suite: **121 passed, zero failures, four
ignored**, recorded in `/tmp/lg-cashu-held-proof-full-tests.log`. These fixtures
use synthetic mint state and do not move real funds. Preparation-before-binding
crash recovery, automatic proof selection/splitting and private treasury service
orchestration remain incomplete.
Strict wallet all-target/all-feature Clippy passed in
`/tmp/lg-cashu-held-proof-clippy.log`; root/wallet formatting, whitespace and both
vendor inventories also passed. No dependency or core-source changes were made
in this increment; the prior full core suite and advisory audit evidence above
remains applicable to those unchanged graphs. Final committed-source CI and
installed/live acceptance are still outstanding.

## Recovery after interrupted treasury preparation binding — 2026-11-16

`Executor::recover_preparation` recovers the boundary after CDK has durably
prepared its melt and before the wallet journal has bound the operation. It
requires the original preparation claim, unchanged quote and exact held proofs,
a matching quote owner and unadvanced `ProofsReserved` saga, and locally retained
mint-scoped fee metadata. The normal owned-output and fee-budget checks bind the
same operation atomically. The recovery method makes no mint request, creates no
CDK operation and submits no melt; the existing resume path remains separate.

A SQLite-trigger fixture interrupts the real journal insertion after CDK
preparation, confirms no binding/submission, then reopens both stores. Recovery
rejects a saga advanced to `MeltRequested` and a quote pointing to another
operation. Restoring the original fixture evidence permits recovery of the exact
original UUID, with one quote attempt and zero melts during recovery. Subsequent
resume uses that original UUID and allows only one submission; another fresh
preparation remains refused and the original claim is retained. All five
executor regressions pass in `/tmp/lg-cashu-preparation-recovery-tests.log`.

This closes the completed-CDK-preparation/before-journal-binding boundary only.
Partial CDK preparation with missing records remains held rather than inferred
safe. Automatic proof selection/denomination splitting, private treasury RPC,
scheduling, deployment acceptance and the rest of the implementation plan remain
unfinished. No production services or financial state changed.

Final full locked/offline wallet suite: **122 passed, zero failures, four ignored**
in `/tmp/lg-cashu-preparation-recovery-full-tests.log`. Strict all-target/all-feature
wallet Clippy passes in `/tmp/lg-cashu-preparation-recovery-clippy.log`.
Root/wallet formatting, both exact vendor inventories and whitespace checks
also pass. Core source and dependency graphs are unchanged from the full-core
and advisory checks recorded above; final committed-source CI remains pending.

## Automatic exact whole-proof selection — 2026-11-16

`Journal::select_treasury_proofs` derives candidates from acquired/returned owned
outputs and their durable source attribution. It requires the core's existing
source reservation, excludes all globally held proofs, and rejects incomplete
attribution. Shared change is eligible only with every source part represented
in the same instruction. Candidates are ordered by descending value then Y;
a bounded exact-subset search preserves each source's exact sat allocation.
The search uses the configured proof limit and a 32,768-node budget. No solution
or budget exhaustion leaves source holds intact and creates no proof hold.
Denomination splitting still requires a separate journaled workflow.

Successful selection passes through the existing atomic ownership/conservation
reservation checks. Its frozen source-to-Y map replays unchanged after restart
and invoice expiry, without issuing a new quote or melt. All real-CDK treasury
fixtures now obtain their acquired proof selections through this method. The
shared-change fixture rejects partial-source selection, reserves one physical
proof for the complete allocation, and replays the same mapping after reopen.
The pure exact selector agrees with exhaustive enumeration across 81 two-source
amount combinations, including mixtures of shared and individual proofs.

Full locked/offline wallet suite: **123 passed, zero failures, four ignored**,
recorded in `/tmp/lg-cashu-proof-selection-tests.log`. Production remains unchanged.
Automatic splitting, partial-CDK-preparation recovery, treasury RPC/scheduling,
installed acceptance and other Cashu plan deliverables remain incomplete.
Strict all-target/all-feature wallet Clippy passed in
`/tmp/lg-cashu-proof-selection-clippy.log`; root/wallet formatting, whitespace and
both vendor checks passed. Core source and dependency graphs are unchanged from
the earlier full-core and advisory verification; final committed-source CI and
installed acceptance remain outstanding.

## Private treasury admission/status transport — 2026-11-16

Added strict shared retain/status frames, wallet UID-authenticated admission and
inspection, and a bounded core client that authenticates the wallet before
sending the frozen instruction. Replies bind request and intent identity/digest;
terminal reports additionally satisfy the existing conservation contract and
are wrapped as kernel-authenticated `PeerMelt` evidence for the core ledger.

The wallet fixture covers disabled new admission, initial retention, lost-ACK
replay through a reopened journal after expiry, status while paused, conflicting
invoice and core-epoch rejection, wrong reply correlation, oversized/unsupported
commands and wrong-peer refusal before frame input. It verifies one retained
instruction, zero source holds and zero quote requests. Full wallet suite:
**124 passed, zero failures, four ignored**, in
`/tmp/lg-cashu-treasury-ingress-tests.log`. A lint-only item-order correction
subsequently moved the journal method ahead of its test module.

The core negative socket fixture requires a wrongly identified wallet to receive
zero request bytes. Positive distinct-UID full-path acceptance, installed socket
configuration, job scheduling and the remaining Cashu implementation plan are
not yet complete. No production endpoint or spending workflow was activated.

Strict wallet all-target/all-feature Clippy passes in
`/tmp/lg-cashu-treasury-ingress-clippy.log`; formatting, both vendor inventories
and whitespace checks pass. Core lint/full-suite verification is in progress
(`/tmp/lg-cashu-treasury-client-core-clippy.log` and
`/tmp/lg-cashu-treasury-client-core-tests.log`); do not treat earlier core passes
as verification of the new client. An initial ambiguous async error type was
corrected before restarting those checks.

## Optional treasury runtime listener — 2026-11-16

The wallet runtime now supervises a separate optional treasury listener. Both
listener activation and new-instruction admission default false. Protected
configuration requires a distinct socket outside state, consistent core epoch,
known per-mint policy bindings and qualified mint exits for new admission.
Paused admission can serve retained status/replays without requiring receiving
toggles or a currently qualified exit. Private status exposes both configured
booleans without disclosing invoices or proofs.

New fixtures cover disabled listener startup, configured read-only status,
continued service after two rejected peers, graceful shutdown, invalid socket/
epoch/mint-policy combinations and qualification gating for new instructions.
These runtime fixtures are pending verification in
`/tmp/lg-cashu-treasury-runtime-tests.log` and
`/tmp/lg-cashu-treasury-runtime-clippy.log` at this checkpoint.
Core strict lint for the preceding protocol/client change passed; its full
suite remains running in `/tmp/lg-cashu-treasury-client-core-tests.log`.

Formatting, whitespace and both exact vendor checks pass for this runtime
increment. The refreshed core audit scanned 317 dependencies against 1,509
advisories with no vulnerabilities; RSA is absent from the active graph
(`/tmp/lg-cashu-treasury-core-audit.log`, `/tmp/lg-cashu-treasury-core-rsa.log`).
Wallet audit used that refreshed database and scanned 331 dependencies with no
vulnerabilities and the existing allowed `paste` unmaintained warning
(`/tmp/lg-cashu-treasury-wallet-audit.log`). Its optional crates.io index access
reported a package-cache lock warning; the lockfile advisory scan completed.
Core full tests are still building; wallet runtime tests are queued on that
same shared Cargo target. Neither pending suite is recorded as passed here.

Runtime verification update: the full wallet suite completed with **126 passed,
zero failures, four ignored** in `/tmp/lg-cashu-treasury-runtime-tests.log`.
Both new runtime/configuration regressions passed. The core suite has also
passed its new wrong-wallet-peer/no-request-bytes fixture, but its full run and
the wallet strict lint are not yet complete at this checkpoint.

## Completed treasury transport/runtime verification — 2026-11-16

The previously running jobs have now completed successfully on the current
transport/client/runtime source:

- Full core locked/all-feature suite: **614 passed, zero failures, eight ignored**
  (`/tmp/lg-cashu-treasury-client-core-tests.log`). This includes 471 library tests,
  the wrong-wallet-peer/no-bytes client regression, and all remaining binary and
  integration suites. Library execution took 393.65 seconds after compilation.
- Full wallet locked/offline/all-feature suite: **126 passed, zero failures, four
  ignored** (`/tmp/lg-cashu-treasury-runtime-tests.log`), including new default-off
  listener, paused admission, configuration validation and rejected-peer liveness.
- Strict core and wallet all-target/all-feature Clippy both passed
  (`/tmp/lg-cashu-treasury-client-core-clippy.log`,
  `/tmp/lg-cashu-treasury-runtime-clippy.log`).
- Formatting, whitespace and both vendor inventories passed; the refreshed core
  advisory gate/RSA absence and wallet advisory results are recorded above.

No verification job from this checkpoint remains running. Positive distinct-UID
treasury admission/replay, durable scheduling, splitting, installed acceptance,
and the remaining plan requirements are still outstanding. These results do not
activate a production endpoint or authorize real payments/feeding.

## Treasury distinct-UID qualification fixture — 2026-11-16

Added `deploy/scripts/test-cashu-treasury-cross-uid.py` and opt-in ignored native
children in the core treasury client and wallet treasury listener. The runner
requires a fresh root-owned network namespace containing only loopback, chooses
unused numeric core/wallet/rogue identities, copies explicitly selected native
artifacts, and creates only temporary state/configuration. The wallet child uses
`Runtime::initialize`, `Runtime::open` and `Runtime::serve_treasury` with public
synthetic keys and locally signed synthetic invoices. The core child uses the
actual authenticated `treasury_client::Client`.

The intended assertions are: wrong UID rejection without blocking the core;
complete request transmission with a lost acknowledgment; exact retry/status;
conflicting invoice refusal; restart preserving the original instruction;
paused new admission while original retry/status remains available; and exactly
one retained instruction with zero source holds, quote requests or melt requests.
The runner compares the fixture instructions across restart and reports SHA256
for both native artifacts. It neither opens installed configuration nor starts
an installed service. This qualifies admission transport only, not a real melt
or independently verified Strike settlement.

CI now runs this fixture with the existing commit-matched core/wallet artifacts
and retains its JSON result with the receipt/signer fixture evidence. Local
execution is pending native core artifact compilation at this checkpoint;
wallet strict lint/build, runner syntax and workflow YAML parsing have passed.
Do not count this gate as passed until the actual distinct-UID run succeeds.

### Treasury distinct-UID result

The actual isolated runner **passed**, recorded in
[cashu-treasury-cross-uid-20261116.json](cashu-treasury-cross-uid-20261116.json).
Core UID 60000 and wallet UID 60001 used the actual core client and runtime
listener; UID 60002 was rejected. Lost acknowledgment, exact replay/status,
conflicting instruction refusal, process restart, and paused admission all
passed. The journal retained exactly one instruction and no source holds,
quote requests or melt requests. Both native artifacts were selected from fresh
successful Cargo JSON, and their SHA256 hashes are recorded in that result.

Strict core/wallet all-target/all-feature lint, native test compilation,
formatting, vendor inventories, runner syntax, workflow YAML parsing and whitespace
checks pass. Production code/dependency graphs are unchanged from the preceding
614-core/126-wallet full-suite checkpoint; this increment adds opt-in ignored
fixture children, their runner and CI execution. Those new children were explicitly
executed by the isolated runner. The two temporary native copies were removed
only after matching their recorded hashes. CI is wired but has not run remotely.

This establishes synthetic admission/status transport across real UIDs. It does
not establish a full mint-to-Strike settlement, production deployment or spending
authorization. Durable job orchestration, denomination splitting, coordinated
recovery and the remaining milestone requirements are still incomplete.

## Wallet treasury worker — 2026-11-16

The full locked/offline/all-feature wallet suite passes **131 tests, zero failures,
five ignored** (`/tmp/lg-cashu-treasury-worker-tests.log`). Strict all-target,
all-feature Clippy passes (`/tmp/lg-cashu-treasury-worker-clippy.log`). The ignored
cross-UID children require their dedicated isolated runners; their earlier results
are separate evidence, not inferred from this suite.

The actual CDK acquisition fixture now drives the worker through source/proof
reservation, quote, preparation and lost-response submission. Reopening with
execution paused permits only original reconciliation. It asserts one quote POST,
one melt POST, retained submitted state, one admission charge, durable backoff and
attempt exhaustion. Journal fixtures separately test stage selection at quote,
preparation and frozen-melt crash boundaries, fair selection between intents,
atomic timestamp overflow, per-stage attempt limits, reopened admission budgets,
clock rollback, changed per-intent limits, and the distinction between expired
24-hour budgets and unresolved in-flight holds.

This fixture deliberately leaves the melt unresolved. It proves no second payment
submission after the lost response, not successful mint/Strike settlement.
Core scheduling/report adoption, denomination splitting, partial preparation
recovery, automatic age/exposure sweeps and installed acceptance remain required.

Additional configuration assertions passed in the focused treasury endpoint test
(`/tmp/lg-cashu-treasury-worker-config-tests.log`): reconciliation without protected
bindings is rejected, omitted switches stay off, and reconciliation with zero
financial limits remains valid while both listener and mint exit qualification
are paused. Formatting, whitespace and exact vendor verification also pass.

## Core treasury coordinator verification in progress — 2026-11-16

Added `core_treasury_worker_reopens_after_lost_ack_and_waits_for_independent_strike`
and `core_treasury_claims_are_bounded_and_uncertain_issuance_never_requeues` to the
synthetic provider fixtures, plus strict default-off control validation. The first
uses an explicitly synthetic wallet transport with authenticated fixture reports;
it is not a real wallet/mint or distinct-UID end-to-end test. It exercises both
paused delivery and paused spending policy after a lost acknowledgment, retained
identity across database reopen, and independent Strike evidence before completion.

The core scheduling table was added to the uninstalled schema-26 draft and strict
additive-upgrade inventory. Formatting, whitespace and both vendor checks pass.
The compile-only job passed (`/tmp/lg-cashu-core-treasury-worker-build.log`).
The full current-source core/migration suite is running in
`/tmp/lg-cashu-core-treasury-worker-tests.log`; new tests and final-source lint
are not yet recorded as passed. Earlier 614-core/131-wallet results do not qualify this new core change.

### Core startup wiring follow-up — verification pending

Added the optional `cashu_treasury` core configuration and supervised daemon task.
The absent/default-off configuration creates no worker. Protected receipt
registration is reused, conflicting sockets or intake identity/epochs are refused,
and global/per-mint financial limits are validated offline. Inspection can remain
configured with receipt import and spending paused; a paused spending policy also
prevents new invoice work claims. Unit fixtures cover unknown fields, default-off
controls, missing registration, mismatched epochs, socket conflicts and invalid
financial limits. The development runbook includes a fully disabled TOML example.

These startup changes were made while the prior full-suite job was compiling;
that job alone cannot qualify the final source. Final-source formatting/whitespace
checks pass. Final-source lint, configuration tests and full suite remain pending.

The overlapping build terminated with E0609: the daemon saw library metadata from
before `cashu_treasury` was added. This is not a passing test run. Source edits are
now paused for a consistent final-source gate; strict lint is running in
`/tmp/lg-cashu-core-treasury-runtime-clippy.log`. The no-fetch advisory scan passed
against the existing 1509-advisory database (317 locked dependencies); optional
crates.io index access warned about its cache lock. The inverse RSA tree is empty.
Logs: `/tmp/lg-cashu-core-treasury-runtime-audit.log` and
`/tmp/lg-cashu-core-treasury-runtime-rsa.log`.

Final-source strict all-target/all-feature Clippy passed in 4m14s
(`/tmp/lg-cashu-core-treasury-runtime-clippy.log`). The fresh full locked/all-feature
core suite is now running in `/tmp/lg-cashu-core-treasury-runtime-tests.log` with
isolated temporary databases and mock loopback services. No source edits are
planned during this run. Full-suite completion remains unproven until its terminal
result is inspected.

Release integration note: this Cashu worktree still lacks the separate
`settle_optional_social` / missing-repost-profile liveness change preserved in
`fix/cyberherd-intake-liveness-20261115` (PR #138). Current Cashu suite results do
not qualify that fix or mean it is deployed. Integrate the reviewed liveness source
before final combined CI/release verification; do not overwrite its financial
ordering/retry protections. The protected production hotfix remains a separately
approved operator step.

### Full-suite failure and migration fixture correction

The consistent-source full run terminated with **470 library tests passed, six
failed, eight ignored** (313.89s after compilation). Both new core treasury
orchestration regressions, control validation and configuration tests passed.
Because the library suite failed, subsequent binary/integration suites did not run.

All six failures were in upgrade fixtures: their historical-schema constructor
retained `cashu_treasury_work`, causing the strict old-schema check to reject it or
migration 26 to report the table already existed. The fixture now removes empty
Cashu additions using the canonical `CASHU_TABLES` inventory, ordered child-first.
The legacy pre-activation comparison uses that same inventory rather than another
copied table list. This preserves strict rejection of unexpected/nonempty state;
it does not bypass migration validation or alter production journals.

The source manifest `/tmp/lg-cashu-core-treasury-runtime-source.json` records the
359-file pre-correction snapshot (SHA256
`b42b65e12dc2543f91981b6a4eba8d8ab9c3d62b7ba383db21b679224a5202d1`). Only
`state_maintenance/cashu_upgrade.rs`, `contact_upgrade_tests.rs` and
`legacy/rows.rs` changed afterward. Focused maintenance tests are now compiling
in `/tmp/lg-cashu-core-treasury-migration-tests.log`; their outcome, updated lint
and the complete final-source gate remain pending.

The focused maintenance run compiled successfully and passed **26 tests**, including
all six previously failing upgrade regressions. Three additional tests failed on
`Operation not permitted` in their isolated root/user-namespace setup; three
explicit fixtures were ignored. These are recorded environment failures, not a
passing suite. The same cached test build is now rerunning with the required
fixture permissions in `/tmp/lg-cashu-core-treasury-migration-qualified-tests.log`.

A separate integration preview at
`/tmp/lg-cashu-upstream-integration-f799qkww/manifest.json` preserves the 26 changed
files from base `54c053b1ae655c829fc6bed96ad59b22b409c082` through reviewed source
`382046cddc5e55d9bf4406013eaf2a75c37ddac3`. It includes the intervening payout
fallback/backup-correlation changes and repost-liveness correction. The status
merge retains both checkpoint sets; the maintenance-code merge retains Cashu
schema checks and incoming fee-probe correlations. It has not yet been applied;
application checks every original file hash before any write.

The qualified maintenance rerun passed **29 tests, zero failures, three explicitly
ignored** in 24.63s (`/tmp/lg-cashu-core-treasury-migration-qualified-tests.log`).
The original six regression failures are resolved. This focused result does not
substitute for the complete core suite.

The reviewed 26-file source integration is now applied, after matching every
original/candidate hash. See
[cashu-source-integration-20261116.json](cashu-source-integration-20261116.json).
It preserves the new Cashu maintenance checks alongside existing payout fallback,
fee-probe backup correlation and optional-social liveness changes. Git history and
production services are unchanged. Combined formatting/whitespace checks pass;
strict all-target/all-feature lint is running in
`/tmp/lg-cashu-integrated-treasury-clippy.log`. The complete combined test gate is
still pending; neither earlier branch's successful tests qualify the integration.

Combined-source strict all-target/all-feature Clippy passed in 3m55s
(`/tmp/lg-cashu-integrated-treasury-clippy.log`). All 17 integrated Rust files
matched the reviewed preview, and the 359-file source snapshot remained unchanged
through lint. Snapshot: `/tmp/lg-cashu-integrated-treasury-source.json`, SHA256
`61fd0406b440aaec343a880c048599f59828cb0a5f77c82a5af47399c38fc374`.
The full combined locked/all-feature core suite is now running in
`/tmp/lg-cashu-integrated-treasury-tests.log`; its result remains pending.

Wallet split-plan fixtures now cover deterministic fee/source allocation,
unassigned remainder preservation, canonical replay/tamper rejection, whole-proof
input and output-count bounds, huge values, unavailable denominations, and
per-source conservation across all permitted preparation fees. The full wallet
suite is running in `/tmp/lg-cashu-split-plan-tests.log`. This does not qualify a
mint swap or recovery path; those remain implementation work. Core source hashes
continue to match `/tmp/lg-cashu-integrated-treasury-source.json`.

The full locked/offline/all-feature wallet run passed **134 tests, zero failures,
five ignored** (`/tmp/lg-cashu-split-plan-tests.log`), including all three new split
accounting regressions. Library tests took 19.74s after compilation. Formatting,
whitespace and exact vendor checks pass. Strict all-target/all-feature wallet lint
is running in `/tmp/lg-cashu-split-plan-clippy.log`. Split execution, durable swap
claims/recovery and integration of preparation costs into melt accounting remain
unfinished; this result proves the arithmetic foundation only.

Strict all-target/all-feature wallet Clippy also passed (56.93s,
`/tmp/lg-cashu-split-plan-clippy.log`). The inspected CDK swap implementation
matches the pinned vendored source byte-for-byte. The new split-plan module
changes no runtime switches and has no swap-submission path.

### Combined core verification and expiry-bound follow-up — 2026-11-16

The integrated Cashu/repost/payout snapshot completed the locked all-feature
core suite: 483 library tests and 143 other tests passed, zero failed; nine
explicitly ignored tests remain. Strict all-target/all-feature Clippy also passed.
All 359 recorded source hashes still matched before the next edit. Per-suite
counts and the log digest are retained in
[cashu-source-integration-20261116.json](cashu-source-integration-20261116.json).

A subsequent correction aligns protected treasury configuration with the shared
intent lifetime range (1–3600 seconds). Its startup regression checks zero, one,
3600, 3601 and 86400 seconds. This subsequent change requires its own verification;
the successful combined snapshot above predates it. No production state changed.

The expiry correction's three focused configuration tests pass in
`/tmp/lg-cashu-expiry-tests.log`; the broader gate runner retains results in
`/tmp/lg-cashu-expiry-gates.json`. Root source changes against the successful
combined snapshot are exactly `src/cashu/treasury.rs` and
`src/cashu/treasury_config.rs`. No root source was edited during this gate run.

Four new wallet journal regressions exercise durable split reservation: original
replay after restart/expiry, changed request refusal, complete transaction rollback,
competing proof ownership, and shared-change provenance including an unrelated
owner's untouched remainder. They use explicitly synthetic already-owned database
records and do not attest CDK swap execution or mint acceptance. Focused results
are pending in `/tmp/lg-cashu-split-reservation-tests.log`; the queued full wallet
gates record results in `/tmp/lg-cashu-split-reservation-gates.json`. Runtime split
execution remains disabled and incomplete.

The four split-reservation regressions now pass (zero failures, 0.18s), and all
recorded wallet source hashes still match the compiled snapshot. The expiry
correction also passes strict all-target/all-feature core Clippy. Full suites are
still pending in their original running gate jobs; no build was restarted due to
waiting for the shared target lock. See
[cashu-split-reservation-20261116.json](cashu-split-reservation-20261116.json).

### Completed expiry/request-binding gates and output-verification follow-up

The core expiry snapshot passes 627 tests (484 library plus 143 others), strict
Clippy, formatting, SQLite vendor and whitespace checks. The wallet original-request
binding snapshot passes 140 tests, strict Clippy and vendor verification. An initial
wallet run had one helper-cancellation timeout; the focused helper and unchanged
full suite then passed without changing the timeout or implementation. Both source
snapshots were hash-rechecked before the subsequent edit. Counts and log digests
are retained in [cashu-split-freeze-20261116.json](cashu-split-freeze-20261116.json).

The next wallet increment extracts the existing derived-output crypto checks for
reuse by read-only split reconciliation and returns a sealed attributed result.
Its regression uses synthetic mint signatures/DLEQ and fresh state, including
missing-evidence and wrong-seed refusals. This is not execution, result commitment
or release of held funds. Its full gates run in
`/tmp/lg-cashu-split-output-{clippy,tests}.log`; those results are pending.

The read-only split verification snapshot now passes 140 wallet tests, strict
Clippy, format and vendor verification; hashes were rechecked before the next
edit. Evidence is retained in
[cashu-split-output-20261116.json](cashu-split-output-20261116.json).
The following atomic accounting increment adds a late-insert rollback fixture,
128 = 100 held + 27 remainder + 1 fee conservation, old-view upgrade/reopen, and
exact-result replay with no extra acquisition/credit. Its gates are running in
`/tmp/lg-cashu-split-accounting-{clippy,tests}.log`; no result is claimed yet.

Atomic split accounting passes all 140 wallet tests, strict Clippy, format and
vendor checks; its source hashes were rechecked before the next edit. Evidence:
[cashu-split-accounting-20261116.json](cashu-split-accounting-20261116.json).
The next increment carries split fees into exact proof selection, quote caps and
original melt preparation/recovery, excluding spent input holds and free remainder.
Its fixture rejects a second fee beyond the original budget and retains only
replacement proof Ys; a source-exhaustion boundary test covers zero remaining
source value. Gates are pending in `/tmp/lg-cashu-split-melt-budget-{clippy,tests}.log`.

Fee-aware melt selection/preparation passes 141 wallet tests, strict Clippy,
format and vendor verification. Its unchanged-source evidence is retained in
[cashu-split-melt-budget-20261116.json](cashu-split-melt-budget-20261116.json).
The next admission increment charges 128 whole-proof sats for the 101-sat intent,
with exact replay, reduced-limit, separate daily count/value/in-flight, rollback
and unresolved day-boundary coverage. Its full gates are pending in
`/tmp/lg-cashu-split-admission-{clippy,tests}.log`; runtime activation is unchanged.

Whole-proof split admission passes 143 wallet tests, strict Clippy, format and
vendor verification; source hashes matched before the next edit. Evidence:
[cashu-split-admission-20261116.json](cashu-split-admission-20261116.json).
The provisional-quote increment tests single-use quote claims, remaining-fee
refusal, missing/expired quote refusal at split freeze, and prohibition on melt
preparation before split reconciliation/selection. Its full gates are pending in
`/tmp/lg-cashu-split-quote-{clippy,tests}.log`. No runtime submission was enabled.

Provisional original-quote ordering passes 143 wallet tests, strict Clippy,
format and vendor verification; source hashes were rechecked before the next edit.
Evidence: [cashu-split-quote-20261116.json](cashu-split-quote-20261116.json).
The next submission-claim fixture exercises missing admission, exact destination/
bytes, combined fee and overflow bounds, expiry, disabled/reduced limits, restart
without a reply, second-claim refusal and exact reply retention. It performs no
network submission. Its gates are pending in
`/tmp/lg-cashu-split-submission-{clippy,tests}.log`.


### Split transport and paused recovery — 2026-11-16

The submission-claim snapshot passed 143 wallet tests, strict Clippy, formatting
and vendor verification; see [its evidence](cashu-split-submission-20261116.json).
The next snapshot connects the one-attempt claim to the real CDK standalone swap
transport. It checks the original reservation, CDK operation ownership, pinned
keys and DLEQ, fresh unspent input states, output derivation, authenticated keyset
fee metadata and original quote affordability before any swap submission.
Separate writer authorities prevent recovery from granting swap, quote or melt
execution permission.

Two actual CDK fixtures acquire 16 net sats as one proof, reserve a 5-sat treasury
allocation, and split it into 4 treasury sats plus 11 remainder sats after a 1-sat
preparation fee. The retained invoice also budgets the future melt input fee.
The lost-reply fixture closes and reopens wallet storage with splitting disabled,
restores the original outputs and verifies one acquisition plus one split, one
quote and zero melts. The receipt digest remains unchanged; custody conversion
creates no second contribution.

Initial compilation caught remaining uses of the old writer lease representation.
The first focused run also caught a fixture assumption about CDK's default
receive denominations. The fixtures now explicitly set their receive proof target
to zero to retain the single large proof; production receive behavior is unchanged.
After these corrections, the full suite passes **145 tests**, with five explicitly
ignored fixture children, strict all-target/all-feature Clippy, formatting and
pinned-vendor verification. Source hashes were rechecked unchanged. Evidence:
[cashu-split-transport-20261116.json](cashu-split-transport-20261116.json).
Logs: `/tmp/lg-cashu-split-transport-final-{clippy,tests,vendor}.log`.

This is library/transport qualification. Automatic bounded split selection,
default-off runtime split configuration, scheduler stages, and complete
split-to-melt-to-independent-Strike settlement coverage remain pending. No live
service, production journal, secret, payment or feeder action was changed.


### Split preparation cancellation — 2026-11-16

Split execution now persists a unique preparation claim before granting the
shared writer's split authority. This closes the interval before CDK creates a
saga or the transport freezes its request: dropping the lease or restarting
cannot allocate a replacement operation. The claim binds the admitted original
reservation and requires the original unexpired, affordable quote. Missing quote
validation does not consume the preparation claim. Existing frozen requests and
melt preparations cannot acquire this fresh-operation authority.

The actual CDK fixture cancels immediately after the claim, verifies rearming is
refused, closes and reopens both stores, and verifies rearming is still refused.
There is no CDK saga, no split, no melt and no extra quote. The original custody
holds remain unavailable for melt selection. This early interruption remains
held; it is not silently classified as a failed operation or assigned another ID.
Submitted splits still use the separately verified original-output recovery path.

All **146 wallet tests** pass, with five explicitly ignored fixture children;
strict Clippy, formatting and pinned-vendor verification pass. Source hashes
match the tested snapshot. Evidence:
[cashu-split-preparation-claim-20261116.json](cashu-split-preparation-claim-20261116.json).
Logs: `/tmp/lg-cashu-split-preparation-claim-{clippy,tests,vendor}.log`.
Automatic bounded selection, split execution/recovery worker integration and
scheduler stages remain pending. No production state changed.


### Reserved split executor and recovery — 2026-11-16

The treasury executor now exposes explicit reserved-split execution and original
split reconciliation. Its existing constructor leaves splitting unconfigured;
`with_split_policy` supplies the separate whole-proof admission policy without
network contact. Execution acquires the durable preparation claim, requires an
exact available local proof inventory and no prior incomplete mint operation,
then calls CDK with the original output denominations. The transport independently
validates ownership, derivation and fee authority before its one POST.

Reconciliation can run with splitting disabled. It binds any remaining CDK saga
to the original frozen request before invoking CDK recovery. Because CDK's public
recovery API also cleans orphaned quote reservations, the executor refuses scoped
mint or melt quote reservations owned by another operation. Independent fresh
input/output state and cryptographic ownership verification precede atomic
custody commitment. A previously committed result replays without mint contact.

Two executor fixtures exercise successful and lost-reply paths across reopening
with splitting disabled. Negative cases preserve unrelated sagas, an altered
original saga, and an unrelated quote reservation. Successful recovery retains
one acquisition plus one split, one quote, zero melts and an unchanged receipt
digest. Both paths select the four treasury sats after the one-sat preparation
fee, then prepare the original quoted melt with one sat of input fee. This does
not submit the melt or prove independent Strike settlement.

The first conflict fixture attempts failed on CDK optimistic version handling;
the fixture now advances saga versions and reloads the quote before restoration.
Two unnecessary Copy clones were also removed after strict lint rejected them.
The final unchanged snapshot passes **148 wallet tests**, five explicitly ignored
fixture children, strict Clippy, formatting and pinned-vendor verification.
Evidence: [cashu-split-executor-20261116.json](cashu-split-executor-20261116.json).
Logs: `/tmp/lg-cashu-split-executor-complete-{clippy,tests,vendor}.log`.

Automatic bounded proof selection, default-off runtime configuration, scheduler
stages, incomplete-CDK-preparation recovery and full independently verified
settlement remain unfinished. No live service or production state changed.


### Automatic split selection — 2026-11-16

`Executor::reserve_split` now derives available candidates from original source
holds, validates local proof ownership/DLEQ and pinned active output keys, and
uses mint-scoped fee metadata to select a covering inventory. Planning uses a
separate writer authority that cannot submit quotes, swaps or melts. Canonical
amount/Y ordering and a 32,768-state iterative search bound the work; whole-proof
exposure, original/local fees, all source parts and output-count limits constrain
every selected plan. Final reservation rechecks global holds and attribution in
the existing atomic transaction. The submission transport still authenticates
fees and custody independently before spending.

New tests compare search feasibility against exhaustive subsets over exposure,
fee and output limits, preserve foreign-owner remainder, and reject missing fee
metadata, excessive fees or unavailable held proofs without partial reservation.
Reopening after invoice expiry replays the original reservation/UUID without
needing new metadata. Both actual CDK executor fixtures now use this automatic
selection before quote creation, then exercise normal/lost-response splitting,
paused recovery and original-quote melt preparation.

An initial compile required an explicit mint URL type. After correction, the
unchanged snapshot passes **151 wallet tests**, five explicitly ignored fixture
children, strict Clippy, formatting and pinned-vendor verification. Evidence:
[cashu-split-selection-20261116.json](cashu-split-selection-20261116.json).
Logs: `/tmp/lg-cashu-split-selection-qualified-{clippy,tests,vendor}.log`.
Runtime configuration, scheduler fallback/stages, interrupted-preparation recovery
and complete independent Strike settlement qualification remain unfinished.
No production service or financial state changed.


### Default-off split scheduler — 2026-11-16

The wallet treasury worker now accepts a bounded nested `split` policy and wires
it to the executor. Effective split execution requires both `execute` and
`split.enabled`; omitted policy stays off. Pausing top-level execution overrides
the nested switch while preserving original split recovery. Private status reports
`treasury_split_execution_enabled` as this combined configuration state.

Exact selection now has a typed unavailable result. Only that result permits
optional split fallback; storage/custody errors and search exhaustion propagate.
The scheduler derives split, recover-split and select-verified-split stages from
durable records, with the existing per-stage claims/backoff/attempt bounds.
Preparation or submission claims always route unfinished splits to recovery.
Committed split outputs must be selected before original melt preparation.

Actual CDK worker fixtures cover a disabled split fallback, enabling within its
retained retry schedule, original quote creation, pause before split, successful
or lost-reply split, worker reconstruction, paused recovery, pause before proof
selection, and resumed original melt preparation. Each observes one acquisition
plus one split, one quote, zero melts and unchanged receipt evidence. Strict
configuration tests cover defaults, required limits and unknown fields.

An initial compile caught a missing mutable fixture journal binding. After that
correction, the unchanged snapshot passes **154 wallet tests**, five explicitly
ignored fixture children, strict Clippy, formatting and vendor verification.
Evidence: [cashu-split-scheduler-20261116.json](cashu-split-scheduler-20261116.json).
Logs: `/tmp/lg-cashu-split-scheduler-qualified-{clippy,tests,vendor}.log`.
Incomplete CDK preparation recovery, complete independently verified settlement,
core financial planning/job generation and installed acceptance remain unfinished.
No production configuration, service or payment state changed.


### Split-to-melt wallet report — 2026-11-16

The scheduler fixtures now continue beyond melt preparation. Their synthetic mint
validates the original quote, input commitments, unique unspent proofs and exact
input-fee conservation before marking the proofs spent. A signed test invoice
uses a known synthetic preimage. The lost-response scenario withholds both the
POST reply and immediate status lookup until worker reconstruction, then permits
paid reconciliation with execution disabled. The first fixture version let CDK
recover inline; withholding status establishes the intended restart boundary.

Both paths report one original quote, one split and one melt. The original five
sats fund three deposit sats and two fee sats (one preparation fee plus one melt
input fee), with zero change. The acquisition receipt digest stays unchanged and
the completed report replays after reopening. Final wallet reconciliation now
revalidates the committed split budget, checks melt fees against remaining
authority, and includes the committed split result in version-two private
operation evidence. Non-split evidence retains its original encoding.

The unchanged snapshot passes **154 wallet tests**, five explicitly ignored
fixture children, strict Clippy, formatting and vendor verification. Evidence:
[cashu-split-melt-settlement-20261116.json](cashu-split-melt-settlement-20261116.json).
Logs: `/tmp/lg-cashu-split-melt-settlement-qualified-{clippy,tests,vendor}.log`.
These tests produce wallet ownership reports; they do not prove independent core
Strike settlement or authorize physical dispatch.

Post-settlement local cleanup remains a separate gap: `Recovery::reconcile`
commits the wallet report but does not clear a retained CDK melt saga, while
acquisition refuses a new operation when an incomplete saga remains for its mint.
Safe original-operation cleanup and a subsequent-acquisition liveness regression
are required. Financial report completion must not authorize a fresh melt or
unconditional release of proofs. Partial preparation recovery, independent core
settlement, financial feed/job planning and installed acceptance remain pending.
No production configuration, service or financial state changed.

The resumed 2026-11-16 dependency gate fetched current RustSec advisories and
passed both locked graph audits plus exact core/wallet vendor checks. The
[evidence](cashu-resumed-dependency-audit-20261116.json) retains the wallet's
non-fatal `paste 1.0.15` unmaintained warning (RUSTSEC-2024-0436). Core's existing
RSA exception remains contingent on its verified unreachable reverse graph;
the wallet audit uses no RSA exception. These checks do not establish native
runtime, deployed mint compatibility, or complete application acceptance.
