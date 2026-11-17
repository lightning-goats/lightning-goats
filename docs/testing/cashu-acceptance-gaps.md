# Cashu full-plan acceptance map — current development gaps

This map follows the complete [implementation plan](../planning/cashu-nutnovember-implementation-plan.md).
It does not redefine completion around passing local fixtures. Evidence is scoped
to its recorded source revision/hash; later edits require relevant requalification.
The canonical summary remains [implementation status](../implementation-status.md).

2026-11-17 intake checkpoint: shared Cashu/nutzap admission now bounds the
uncredited backlog to 1024 acquisitions and 16 MiB of serialized input proofs,
and retains at most 64 source aliases per acquisition. Full queues preserve
existing retries; completed credits free capacity while preserving history.
[Wallet qualification](cashu-intake-backpressure-20261117.json) passes 168 tests
with seven explicitly ignored fixture entry points, formatting, strict Clippy,
vendor verification and cached dependency audits. This does not resolve the
separate mint-operation fence after rejected/ambiguous acquisition, provide a
total database quota, or qualify production receiving.

The subsequent [mint-progress checkpoint](cashu-mint-progress-20261117.json)
allows an independent receive after CDK has compensated a definitive rejection,
while retaining original recovery records and checking reserved/disjoint output
counter ranges. All 170 wallet tests pass with seven explicitly ignored fixture
entry points, formatting, strict Clippy, vendor verification and the cached
dependency audit. Counter rollback and missing recovery material fail closed;
unfinished CDK work still waits for original-operation recovery. This is
synthetic-provider evidence, not live mint or complete recovery qualification.

The [2026-11-17 notice correction](cashu-license-authors-20261117.json) reduces
missing notices from 17 to 15 by recognizing the full MIT permission/copyright
notice already present in both `r-efi` archives' `AUTHORS` file. Exact upstream
blob comparisons and 13 archive/supplement tests pass. Dependency coverage remains
462 packages with no unresolved sources; distribution readiness remains false.

The [HTTP retry checkpoint](cashu-http-retries-20261117.json) now passes all
648 core tests with 12 explicitly ignored fixture entry points, formatting,
strict Clippy and cached dependency checks. Its explicit separate-UID runner
passes 80 identical HTTP retries for each of two distinct payments to the same
request, retaining exactly two intake-source records and stable pending receipt
capabilities. Paused restart still permits status lookup while refusing new
admission. This qualifies admission/retry transport, not acquisition or credit.

The [current-artifact financial matrix](cashu-final-composition-20261117.json)
now passes eight full HTTP/receipt/treasury IPC cases with real CDK and the pinned
offline Cashu-TS sender: zero-fee, fee-only, returned-change and repeated-request
modes, each with normal and lost melt replies. Original treasury IDs and credit
remain stable; internal transfers create no extra contribution. Returned dust
still needs a later one-sat native contribution in that particular fixture.
The later residual-change and multi-mint checkpoints below extend this matrix.
Real providers, independent phone clients and physical acceptance remain open.

The operator confirmed a backed-up, new dedicated 12-word seed. The
[offline seed-preparation script](../deployment/cashu-seed-provisioning.md)
passes eight public/synthetic tests, including real terminal echo suppression
and private atomic file creation. The operator confirmed seed staging; metadata-only observation found a private
64-byte file under a 0700 directory. The phrase was not supplied to Codex and the
seed is not installed into a runtime. The receiving key and NIP-46 credentials
remain separate.

The [private operational health checkpoint](cashu-private-health-20261117.json)
passes 175 wallet tests (seven explicitly ignored fixture entry points), strict
Clippy/format/vendor/dependency gates and eight explicit financial composition
cases with health-accounting checks. It reports journal custody/fees, queue age
and schedule lag, legacy unknown times, future timestamps, retained core epochs
and fenced wallet identity mismatches without reading keys or exposing proofs.
Global totals include historical mints beyond the detail limit. These are
private diagnostics; provider balances and installed runtime health remain separate.

The [protected operator-controls checkpoint](cashu-operator-controls-20261117.json)
now passes 650 core tests (12 ignored), 179 wallet tests (seven ignored), all
required local gates and both actual native CLI fixtures. Settlement preview
rolls back source/request reservations; apply/restart retains one original
intent, while bounded approvals bind exact configuration and core/wallet scope.
The wallet command checks only one original acquiring/acquired operation under
its writer lock, without swap/melt replay, hold release or receipt export.
All eight [financial composition cases](cashu-operator-composition-20261117.json)
pass on the new source-matched artifacts. CLI fixtures use empty/unfunded
synthetic state, with systemctl mocked only for the core fixture. Funded native
ledger and actual-CDK ownership tests provide their separate boundaries.
Installed acceptance, held/policy and treasury controls, coordinated funded
restore and independent client/provider acceptance remain required. Later
checkpoints below qualify synthetic multi-mint and economical-change composition.

Public Cashu development is now in [draft PR #139](https://github.com/lightning-goats/lightning-goats/pull/139).
The missing published `rusqlite` lockfile discovered by clean-checkout CI is
retained exactly at `c36df2c`; fresh-export vendor and 13 notice tests pass.
The [economical residual-change composition](cashu-residual-settlement-20261117.json)
now passes normal and lost-reply cases in selected-source CI at `69ba9de`.
It combines retained one-sat change with a distinct 16-sat ordinary Cashu
contribution admitted before the first conversion and excluded from its original
plan. Two independently verified Strike receives total 413 sats, preserve 1016
sats of contribution credit, and permit one synthetic 1000-sat feeding with
16 credit left. The transfers add no donation or credit. Wallet health verifies
zero remaining custody and three sats each of receive/conversion fees. This
does not test admission after the first melt or real providers. The deployment
rehearsal now passes the strict contact comparison. It reaches a later historical
payout-8 source-verifier failure on core schema 26. The explicit reviewed
empty-Cashu source variant now passes all ten selected-source workflows at
`cb582e9`, including the installed native/systemd transaction
([evidence](cashu-upgrade-compatibility-20261117.json)).

The `--multi-mint` composition case now passes normal and lost-reply execution
at `0f66d53`, with all ten selected-source workflows successful
([evidence](cashu-two-mint-settlement-20261117.json)). It acquires 250 net sats
from a nutzap at one configured issuer and 150 net sats through ordinary HTTP at
another. Independent issuer keys, keysets and CDK namespaces share one journal
and writer. Both original Strike receives settle independently before one
synthetic feeding. Health verifies 396 sats deposited, four sats of conversion
fees, two sats of receive fees, zero remaining custody and no new transfer credit.
Lost replies and consumed-credit replay preserve those totals. These synthetic
providers and the offline SDK sender do not qualify installed Runtime, live
mint, Android, relay, TLS or physical acceptance.

| Required behavior / artifact | Established evidence | Required work still open |
| --- | --- | --- |
| Existing native Lightning, NIP-57 and paid history | Core regression suites and historical live checkpoints; no production change in this workstream | Final selected-source regression and approved installed/provider acceptance. |
| Ordinary Cashu and NIP-61 common acquisition | Actual CDK acquisition, shared proof identity journal and cross-transport validation tests; full loopback HTTP-to-acquisition-to-financial-IPC fee-only composition passes normal/lost reply ([evidence](cashu-http-financial-ipc-20261116.json)) | Independent sender applications, rejected/held sender interoperability and live relay acceptance; multiple acquired payments per descriptor now pass the SDK-assisted fixture below. |
| Reusable amountless strict-mint requests | Native CLI generation and deterministic IDs; original creqB parses in Cashu-TS; standalone amountless creqA correction parses | Integrated corrected binary generation, all request/browser requalification, actual sender scan selection. |
| Several distinct payments to one request; wrapper/relay replay | Acquisition/receipt overlap tests and exactly-once imports; the [eight-case financial matrix](cashu-final-composition-20261117.json) includes an offline SDK sender, repeated descriptor use and lost melt replies; [HTTP retries](cashu-http-retries-20261117.json) retain two source entries after 80 identical retries per payment | Independent sender applications, TLS/phone interoperability and live relay discovery/replay acceptance. |
| Truthful asynchronous public outcomes | HTTP 202 follows authenticated durable admission and reports pending with a body-only status capability. The [positive HTTP/snapshot checkpoint](cashu-request-acceptance-20261116.json) and [retry checkpoint](cashu-http-retries-20261117.json) cover malformed-before-valid admission, two distinct payments, repeated delivery and paused-restart status | Independent sender status/capability interoperability and installed ingress/privacy acceptance. A pending response still does not claim acquisition or credit. |
| DLEQ/P2PK, mint/keyset/amount/condition bounds | Locked library-backed validation and synthetic protocol tests | Final independent-client compatibility and approved live mint policy/capability evidence. |
| Multiple configured mints and key rotation/removal | Configured registry, mint-scoped journals and historical binding tests; mint-scoped CDK collision/recovery regression and [two-mint HTTP/financial composition](cashu-two-mint-settlement-20261117.json) pass normal/lost replies. Same-key [descriptor retirement](../deployment/cashu-wallet-development.md#retiring-a-published-request) has local window, mint-scope and exact retry coverage. | Historical receiving-key rotation, explicit legacy custody migration and deployed recovery; independent client/provider acceptance. |
| Actual swap/melt recovery and ownership | Lost-reply, quote adoption, partial preparation, split and cleanup fixtures | Earlier preparation boundary recovery, full process/CDK-store restart coverage at all required boundaries, full coordinated restore. |
| Atomic authenticated shared-credit import | Cross-UID lost-ack and consumed-credit replay tests; [full HTTP/receipt/treasury composition](cashu-final-composition-20261117.json) with original contribution IDs and no transfer credit | Final selected-source cross-UID regression and approved installed/provider acceptance. |
| Mixed 600 LN + 250 nutzap + 150 Cashu | Actual CDK plus core composed financial fixtures; authenticated receipt and treasury sockets across distinct UIDs pass normal and lost-response fee-only modes ([evidence](cashu-combined-financial-ipc-20261116.json)) | Public HTTP normal/lost-reply fee-only composition now passes on matched artifacts ([evidence](cashu-http-financial-ipc-20261116.json)); independent sender/TLS and separately approved real-provider acceptance remain open. |
| Mixed nonzero fees and returned change | Actual-CDK ownership/change, core source coverage and conservation checks pass in the [eight-case financial matrix](cashu-final-composition-20261117.json); [economical residual settlement](cashu-residual-settlement-20261117.json) and [two-mint composition](cashu-two-mint-settlement-20261117.json) pass normal/lost replies with health accounting and no transfer credit | Independent client/provider acceptance and complete funded restore remain open. The earlier returned-dust fixture alone does not establish economical settlement. |
| Early/duplicate treasury webhook without donation credit | Core purpose-bound receive tests and combined lost-response fixture | Re-run with final selected source, fee/change and complete transport composition. |
| Pending one mint, later sufficient Lightning; paused/unknown feeder | Financial plans, supersession, allocation and ledger-only feed fixtures | Composed multi-mint recovery, operator tooling and installed physical-boundary acceptance. |
| Sweep races and durable proof reservations | Qualified age/balance/operator sweep fixtures, fair-mint admission, protected core settlement preview/apply/replay and normal/lost-reply two-mint composition | Deployed scheduling/operator acceptance and complete multi-mint recovery. |
| Caps, retirement, excess custody and stable credit | Core financial/retirement and wallet conservation tests | Full selected-source cap/fee/change composition, private exposure health and old-policy recovery. |
| Cashu must not create Strike payout authority prematurely | Reward provenance remains native-only; internal transfers create no donation grant | Optional Cashu-derived reward support stays explicitly disabled/unqualified. Enabling it requires its separate provenance acceptance. |
| Isolated identity/key/signer boundaries | Real separate-UID receipt, signer and treasury fixtures; inactive unit syntax | Installed credentials/ownership/egress/systemd acceptance and final source-pinned signer checks. |
| Private descriptor IPC, HTTP and overlay snapshots | Pre-correction increment passes 637 core/156 wallet tests and all gates | The [positive separate-UID HTTP/snapshot checkpoint](cashu-request-acceptance-20261116.json) passes; keep final selected-source and sender-status composition qualification separate. |
| Requested two-QR layout at 1080p/720p | 44 browser cases, long-request decoding, card/banner/effect collisions | Corrected native output requalification, actual wallets and physical phone scans. |
| Edge CORS, body limits and log privacy | Four real isolated nginx tests; application CORS/default-off tests | Installed CDN/WAF suppression, real browser-wallet cross-origin POST/OPTIONS, sender retry behavior and ingress abuse acceptance. |
| Protected operational commands and useful private health | Wallet config/status/request/announcement commands; private custody/fee/age/lag/identity diagnostics; protected financial release, core settlement planning and one-original-acquisition ownership check with explicit native/CLI qualification | Held/mint-policy administration, treasury reconciliation, coordinated backup/restore commands and installed operational acceptance remain incomplete. |
| Coordinated backups and rollback fences | Actual native recovery fences and initialized combined inventory pass. The [coordinated collector](cashu-coordinated-collector-ci-20261117.json) passes native database/key checks, age roundtrip, original-byte preservation, four collector/six credential refusals and encryption size/timeout faults in selected-source CI. Service-status responses and the maintenance-lock path are mocked. A new local fixture stages the encrypted checkpoint for all six roles and verifies committed WAL content and recovery markers. | Installed writer quiescence and complete credential coverage, portable export/reprovisioning, treasury/CDK ownership reconciliation, external outcome verification, installed recovery and resumption, historical-key administration and off-host retention remain open. |

The [funded rollback fixture](cashu-funded-rollback-fences-20261117.json) captures core and wallet images after the original
invoice is bound but before the melt. It then loses the melt response, verifies
the original Strike deposit and records one synthetic feeding. It compares
wallet-only, core-only and coordinated stale combinations and exercises the
production startup fences on the older images. At `fa15df3` it passes all ten
workflows. The older wallet retains 400 sats in the CDK database. A later
wallet report and Strike deposit show which original melt spent them. The
wallet-only rollback fails retained history checks, while a stale core can
still agree with either wallet generation. Every older image remains fenced.
Installed recovery, external outcome reconciliation after restore and fence
clearance remain open.
| Inactive install/package, current docs and pinned dependencies | Inactive unit/edge examples; pinned CDK/SQLite vendor verification; initial November inventory | Full install/package/network/backup rehearsal and selected-commit CI. The operator skipped the security review on 2026-11-17; the scan was canceled before validation/completion and supplies no completed-review claim. |

The [native copy-normalization primitive](cashu-copy-normalization-local-20261117.json)
now passes local root/network-isolated main/WAL and interruption/refusal checks,
including implicit row IDs and signed-byte preservation. It uses the pinned
SQLite backup API on protected copies and retains incomplete evidence. Fresh
selected-source CI is pending; [coordinated collection](../deployment/cashu-backup-collection.md),
encryption, funded rollback and recovery-fence clearance remain unfinished.
| Public November package and demo | Pre-November and Cashu starting references recorded; fixture evidence retained | Reviewable public commits/PR/tag, distribution license notices, current submission rules, compatibility matrix and honestly labeled recording. The checksum-verified inventory covers 462 entries from both lock graphs with no unresolved entries; it is not a linked-binary or distribution-notice inventory. |
| Approved live pilot and production | No activation claimed | Operator-selected mints/keys/limits/fees/holding policy and explicit approval; real providers, independent clients, actual physical evidence and observation. |

## Source checks behind newly recorded gaps

The earlier `src/cashu/http.rs` exposed token POST and public request GET with no
sender status capability. The next integrated increment now adds body-only
capabilities and status POST; its full application acceptance remains pending. A 202 admission is not proof that
owned outputs were acquired or the core credited them. Do not expose the private
acquisition ID as an authorization token or add capabilities to URLs/logs.

`LedgerStore::release_financial_feed_plan` requires an exact plan/threshold,
retains custody work, and refuses unresolved physical attempts. It now has a protected command in `src/bin/lightning-goatsctl.rs`, with preview
by default and an exact epoch/plan/threshold approval. Source integration alone
does not qualify protected operator execution; its root/file/CLI checks are
still required, retaining the exact plan and original treasury identities.

These gaps are implementation work, not reasons to change real financial or
physical settings. Production HOLD and explicit operator steps remain in force.

## Resumed qualification observations — 2026-11-16

The nonzero-fee combined fixture currently tests 398 sats deposited, one sat
conversion expense and one sat retained change from the 400-sat wallet holding.
It preserves the original credit, then adds a separate one-sat Lightning
contribution to dispatch while the change remains unsettled. That is useful
conservation/later-native-progress coverage; it is not full evidence that the
original mixed contribution finishes settlement and dispatch without another
donation. Qualify a separate nonzero-fee/no-returned-change case proving the original
1000-sat credit is consumed once without manufactured funding. Also qualify
economically viable remaining-change settlement; a one-sat holding facing a
one-sat mint input fee must remain held rather than be written off. Source
inspection confirms the planner accounts for returned change and permits
subsequent jobs; the dust fixture alone does not establish a planner defect.

The protected approval runner now requires exact native test names and the
positive root protected-file marker. It rejected the older binary in an isolated
network namespace ([evidence](cashu-financial-release-old-artifact-20261116.json)).
Positive current-binary execution passes all three root/netns approval checks ([evidence](cashu-financial-release-approval-20261116.json)); the actual guarded CLI also passes isolated preview/refusal/apply/retry checks ([evidence](cashu-financial-release-cli-20261116.json)). Installed-systemd acceptance remains open.
CI includes both nonzero-fee modes and this explicit root approval check.

The resumed core source has passed formatting and strict Clippy; full native tests are still running and the wallet verifier is queued behind it. Fresh dependency audits passed for both locked graphs, with the existing core RSA exception checked as unreachable and the wallet `paste` unmaintained warning retained ([audit evidence](cashu-resumed-dependency-audit-20261116.json)). Staged transport, fee-matrix and membership integration changes are not covered by those results.

## Integrated native checkpoint

The wallet gates now pass: 154 library and seven other native tests, seven
explicitly ignored, strict Clippy, formatting and vendor checks; source hashes
remain unchanged ([evidence](cashu-integrated-wallet-result-20261116.json)).
The [separate-UID HTTP fixture](cashu-integrated-http-status-20261116.json)
passes two distinct pending payments, malformed-before-valid admission, replay,
capabilities and paused-restart status; it does not acquire or credit funds.
The [fee-only composition](cashu-integrated-fee-only-20261116.json) passes both
normal and lost-reply modes with original credit consumed once and no extra
donation. It uses real CDK with synthetic providers and simulated confirmation;
complete private-IPC financial composition and real-provider acceptance remain
open. The full core rerun is still running in an unprivileged loopback namespace.

The matched-artifact [composition matrix](cashu-integrated-composition-matrix-20261116.json) now passes zero-fee and signed-change cases, each with normal/lost replies. Together with the fee-only evidence above, all six variants pass. The one-sat signed-change fixture retains that uneconomical change and uses a later one-sat native contribution; it does not establish economical residual-change settlement or multi-mint composition.

The final-artifact [IPC matrix](cashu-integrated-ipc-matrix-20261116.json) passes separate-UID receipt, signer and treasury fixtures, including their retry/restart and identity checks. These are individual adapter fixtures; complete public HTTP-to-acquisition-to-receipt-to-settlement composition remains open. No installed credential/ownership/egress or real-provider acceptance follows from these results.

All eight existing [root-owned maintenance fixtures](cashu-integrated-root-matrix-20261116.json) pass on the matched core artifact in fresh network namespaces. These preserve the existing maintenance boundaries; they do not add coordinated Cashu restore support. Remote selected-commit CI and the guarded financial-release CLI remain unqualified.

The [actual CLI fixture](cashu-financial-release-cli-20261116.json) now passes
preview, simulated active-service refusal, one-time apply/retry and runtime-owner
checks using native migrations and an isolated ledger. Its mount/network
namespaces replace systemctl responses locally; installed systemd acceptance and
release with real pending custody remain unqualified. CI now invokes this check.

The new [two-mint regression](cashu-multi-mint-storage-defect-20261116.md) confirms a shared-CDK-store recovery defect for equal proof secrets across independent issuers. The original shared-store collision failed while the distinct-secret control passed. Mint-scoped CDK storage now corrects that defect, and both actual-CDK two-issuer tests pass. The runtime/health gates pass 166 native tests with seven ignored ([evidence](cashu-namespace-health-20261116.json)); retained legacy custody and incomplete namespaces fail closed. Full multi-mint settlement, explicit legacy migration and coordinated restore remain unqualified.

The [combined receipt-socket checkpoint](cashu-combined-receipt-ipc-20261116.json) passes normal and lost-melt-reply fee-only cases with actual CDK acquisitions, distinct-UID receipt delivery, acknowledgment checkpoint retry and replay after consumed credit. Treasury instructions/reports remain file-coordinated in that checkpoint. The new treasury socket extension is integrated and under compilation, not yet qualified; public HTTP intake is still absent from the combined fixture.

## Current transport staging checkpoint

The treasury socket extension remains under its source-frozen core/wallet gates.
The next public HTTP composition is reviewable in
`.artifacts/cashu-combined-http-stage/`, with original and staged SHA-256 hashes.
Its formatted Rust and Python syntax checks pass; it is not integrated or
native-qualified. It routes the ordinary contribution through HTTP and wallet
intake before the same CDK, receipt and treasury paths, and checks durable status
and replay after consumed credit. It uses loopback HTTP, a synthetic mint and
mocked Strike; independent senders, TLS, live relays and physical effects remain
outside its evidence. Apply only after the treasury IPC modes are qualified.

The first combined treasury IPC execution failed because the fixture returned
an authenticated Status reply under the original Retain request's correlation.
The core rejected it without accepting a completion. The fixture now retries
the exact original Retain after observing the retained report; native v2 core
qualification is pending. See
[captured failure](cashu-treasury-ipc-correlation-failure-20261116.json).
Wallet gates passed 166 tests with seven ignored
([result](cashu-treasury-ipc-wallet-result-20261116.json)); that does not qualify
the combined mode. The staged HTTP source includes the same correlation fix.

The [exact-upstream notice supplement](cashu-upstream-notice-supplement-20261116.json)
now supplies seven missing registry-package notices, matching their crate-recorded
upstream commits and Git blob hashes. The v4 development export has 462 entries
and 23 missing notices, with distribution readiness still false. Six boundary
validation tests pass. The archive inventory remains an accurate record of
which release archives omitted notices; the supplement is explicit.

The v6 notice export supersedes the v4 count above: eleven exact-commit registry
package supplements now cover the CDK registry crates, Valuable, three LDK
crates and Wasite. There are 19 missing notices among the same 462 entries.
Both text SHA-256 and Git blob IDs match. The exporter refuses incomplete
output without the development flag and creates no output on that refusal.
Six supplement boundary tests still pass; distribution readiness remains false.

V7 notice checkpoint: twelve exact-commit registry packages supplemented,
18 notices still missing, eight supplement tests passing. Gloo's pinned
commit remains available after its repository redirect; original and retrieved
URLs are both retained. The current export remains development-only.

Backup inventory filesystem regression: an unlisted FIFO previously passed
verification. The verifier now rejects all nonregular/nondirectory entries
and directory traversal failures. Sixteen tests pass, including FIFO/socket
and unreadable-directory refusal
([evidence](cashu-backup-special-entries-20261116.json)). Coordinated creation,
restore and external ownership reconciliation remain open.

## Combined financial IPC checkpoint

The corrected treasury fixture now passes both normal and lost-melt-reply modes
on matched native artifacts. The core uses the actual receipt server and
treasury client; the wallet uses actual CDK acquisitions, receipt delivery and
treasury ingress. The final retained-result reply comes from an actual retry
of the original Retain request. Three original contributions grant 1000 credit
sats, two receive-fee sats remain explicit, the wallet deposits 398 sats with
two conversion-fee sats, and simulated confirmation consumes the original
credit once. No supplementary contribution or transfer-derived credit occurs.
The lost-reply mode independently verifies Strike before the wallet report,
then recovers while wallet acquisition is disabled.

[Financial IPC evidence](cashu-combined-financial-ipc-20261116.json) records the
artifact/source hashes and gate scope. Providers and physical confirmation are
synthetic. This checkpoint has no public HTTP intake. The next HTTP extension
is now integrated and under new native gates; it checks descriptor metadata,
malformed-before-valid POST, durable admission/replay capability, actual
acquisition/import and credited status/replay after consumed credit. CI includes
normal/lost-reply variants, but those HTTP modes have not passed native acceptance.

V8 notice checkpoint: thirteen packages supplemented, including an explicit
local CDK origin binding, with 17 missing notices among 462 entries. The local
CDK binding requires the existing complete vendor/patch/lock verifier plus its
inventoried manifest and exact original archive. It remains a local source in
the export. Ten notice tests pass; incomplete output still requires the
development flag and distribution readiness remains false.

An actual-CLI empty-state backup fixture is now implemented, syntax-checked and
wired into CI, with native execution pending
([preparation](cashu-native-backup-preparation-20261116.json)). It includes
core/wallet/signer bindings and two native CDK namespaces. It must not be used
as evidence for funded restore, post-snapshot financial activity or external
ownership reconciliation.

The backup verifier now streams component hashes/copies instead of allocating
an entire ledger image; its 32 MiB memory regression and all seventeen
inventory tests pass ([evidence](cashu-backup-streaming-20261116.json)).
HTTP core gates have passed, but the wallet fixture initially failed compilation
because Reqwest JSON helpers are disabled. The fixture now uses explicit Serde
bodies/response decoding, with fresh wallet gates running; combined HTTP
acceptance remains unqualified.

## Public HTTP financial checkpoint

The full composed HTTP mode now passes normal and lost-melt-reply cases on
matched native artifacts ([evidence](cashu-http-financial-ipc-20261116.json)).
A malformed POST is rejected before the valid ordinary contribution. That
contribution enters through the actual public HTTP router, authenticated wallet
intake and journal, actual CDK acquisition, distinct-UID receipt delivery,
treasury Retain/Status exchange and independent mocked Strike verification.
Status and repeat POST preserve the original capability/credited result even
after the original feed credit has been consumed. The earlier validated
synthetic nutzap uses the same acquisition engine directly.

This is one ordinary contribution in the mixed 600/250/150 scenario, with
nonzero fees and no change. It does not establish multiple acquired ordinary
payments to one descriptor, an independent sender application, actual relay
delivery, browser/TLS/provider acceptance or physical actuation. Core gates
compile the test artifact and run protocol/vendor/audit checks; this checkpoint
does not substitute for a final full core suite. The full wallet suite passed
166 native tests with seven ignored. Actual core/wallet/signer CLI builds for
the separate empty-native-backup fixture have started sequentially.

The [actual-CLI empty native backup fixture](cashu-native-backup-result-20261116.json)
now passes: three native programs, distinct runtime UIDs, eleven snapshot
components, two CDK mint stores, matching registrations, missing key/store and
wrong-epoch refusals, and unchanged wallet bytes behind the recovery marker.
No services were installed or started. Funded restore and external outcome
reconciliation remain unqualified; this initialized-state result does not
close those requirements.

The [Cashu-TS SDK-assisted ordinary sender](cashu-sdk-sender-20261116.json)
passes normal and lost-melt-reply full HTTP/financial-IPC modes. It uses actual
Cashu-TS cached-wallet offline proof selection, amountless native request parsing
and SDK integer serialization, plus Origin/OPTIONS and lost-admission-body
retry. Six pinned-code inventory tests pass. This closes a library sender
interoperability slice; independent end-user wallet applications, actual browser
and phone scans, multiple acquired payments per reusable descriptor, live NIP-61
relay sending and approved real providers remain open.

The next composed mode, `--repeat-request`, is implemented under fresh native
gates. It keeps one descriptor while acquiring two distinct 75-sat ordinary
payments alongside the 250-sat nutzap and 600-sat native contribution. It
requires distinct capabilities, then queries and replays both payments after
credit consumption. The larger owned-proof set has a three-sat conversion
budget and 397-sat expected deposit; original granted credit remains 1000.
The preparation record is
[cashu-repeated-request-preparation-20261116.json](cashu-repeated-request-preparation-20261116.json).
Native acceptance has not passed; this pending fixture does not close the
multiple-acquired-payment requirement. CI includes baseline and repeated SDK
sender modes, each normal and lost-melt-reply.

Native core/wallet/Cashu-broker recovery fences now pass actual CLI checks
([evidence](cashu-native-recovery-fences-20261116.json)), including a positive
unfenced broker startup control and unchanged bytes on refusal. Core reuses
the existing `CYBERHERD_RECOVERY_REQUIRED` marker; wallet and Cashu broker use
`CASHU_RECOVERY_REQUIRED`. Funded rollback remains open. The partial Cashu
inventory also does not include the existing membership, payout and payout-
signer journals: full recovery must extend the existing four-journal contract
and keep both signing roles distinct. Do not treat the eleven-component
empty-state fixture as that complete combined inventory.

## Multiple acquired payments per reusable descriptor

The [four-case matrix](cashu-repeated-request-result-20261116.json) now passes
its two baseline SDK cases and two repeated-descriptor cases, normal/lost melt
reply. Two distinct ordinary 75-sat payments use one native descriptor and
produce distinct original capabilities, actual acquisitions and receipt imports.
Both status capabilities and POSTs are replayed after simulated feed consumption
without new swaps, contributions or credit. Three Cashu acquisitions retain
400 net sats after three receive-fee sats; their 397-sat deposit plus three
conversion-fee sats preserves original 1000 granted credit and one simulated
confirmed feed. No supplemental native donation is needed. This closes the
isolated acquired-payment/replay scenario, not independent end-user wallet,
TLS, real browser/phone, live relay/provider or physical acceptance.

The backup foreign-key regression now refuses orphaned internal rows that pass
SQLite integrity checks; eighteen tests and the actual native schema/fence
fixture pass ([evidence](cashu-backup-foreign-keys-20261116.json)). Complete
existing-CyberHerd correlation and funded restore remain open.

## Current native recovery correlation increment — 2026-11-16

A distinct read-only `review-cashu-correlations` command now links the existing
four-journal native verifier to a Cashu inventory by digest and identical core
image. Its report cannot authorize restore or resumption and does not claim
wallet verification. Ordinary maintenance still refuses Cashu receipts. A new
explicit ignored root fixture preserves actual native Cashu rows, verifies all
four journals and checks malformed bindings, unchanged images, ordinary restore
refusal and a rehashed wrong membership epoch. The selected-artifact root runner
and CI step are prepared. This source increment is **unqualified** until current
Rust gates and the explicit root fixture pass. The combined collector/verifier,
funded rollback and external reconciliation remain open.

## Initialized combined recovery checkpoint — 2026-11-16

The [standard source gate](cashu-correlation-standard-gates-20261116.json) passes
648 tests with 12 ignored fixture entry points, strict Clippy, formatting,
protocol/vendor checks and the cached advisory audit. The distinct root Cashu
correlation fixture reached its positive review and refusal assertions, then
failed because its negative setup attempted to rewrite a native immutable core
binding. [Failure evidence](cashu-correlation-fixture-failure-20261116.json) is
retained; a separately initialized different-epoch journal now replaces that
invalid setup. Only the ignored test changed; current corrected-source gates
are running. No complete root-fixture acceptance is claimed yet.

The [initialized native combined rehearsal](cashu-empty-combined-native-backup-20261116.json)
passes with actual core, wallet, Cashu broker, membership, payout, payout signer
and statectl CLIs under six distinct runtime UIDs. Fourteen Cashu inventory
components and two mint stores agree with all four native journals and identical
core bytes. Substituting the Cashu broker for the payout signer refuses. Existing
wallet/core/Cashu-broker startup fences refuse and preserve bytes. This remains
empty-state evidence: funded rollback, historical keys, scoped signer-client
credential recovery, production writer quiescence and external reconciliation
are unfinished, and every restore/resumption authorization remains false.

The [eight existing root maintenance fixtures](cashu-correlation-maintenance-regressions-20261116.json)
also pass against the standard-gate executable. Their fixture bodies and all
production code are unchanged by the separate Cashu negative-setup correction.
This preserves ordinary four-journal/WAL backup, fenced restore, upgrade,
retirement and cutoff behavior. The [corrected Cashu root fixture](cashu-correlation-root-result-20261116.json)
passes separately on its newly compiled executable, and the
[full corrected-source rerun](cashu-correlation-corrected-gates-20261116.json) passes
648 tests with all required local checks.

The corrected explicit root correlation fixture now passes its positive marker
with an actual native Cashu receipt and all four journals, malformed scope/image
refusal, unchanged input bytes, ordinary maintenance/restore refusal and a
separately initialized different-epoch membership image. The immutable binding
trigger remains intact. This is native four-journal correlation evidence, not
Cashu ownership verification, funded restore or resumption authority. Evidence:
[corrected root result](cashu-correlation-root-result-20261116.json).

The [corrected-source gate](cashu-correlation-corrected-gates-20261116.json) is now
complete: 648 passed, 12 deliberately ignored fixture entry points, strict
Clippy/format/protocol/vendor/whitespace checks and the cached advisory audit.
Explicit root Cashu correlation, the eight existing maintenance fixtures and
initialized combined native CLI checks have separate positive evidence. None of
these closes funded rollback, full key/client recovery, writer-quiescent backup
collection, external reconciliation, multi-mint financial composition or the
remaining operator/client/deployment requirements.
