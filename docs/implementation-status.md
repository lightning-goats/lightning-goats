# Lightning Goats implementation status

Cashu publication update **2026-11-17**: public publication is authorized and
[draft PR #139](https://github.com/lightning-goats/lightning-goats/pull/139) is open.
The fully qualified membership repair [#138](https://github.com/lightning-goats/lightning-goats/pull/138)
has merged; the Cashu draft now targets main. Fresh CI exposed an omitted upstream
`rusqlite` archive lockfile. Commit `c36df2c` retains the exact published bytes;
a fresh Git export passes vendor verification and all 13 notice tests. Core and
wallet CI gates now pass for that checkpoint; combined/other CI qualification is
recorded separately. The [two-settlement residual-change composition](testing/cashu-residual-settlement-20261117.json)
passes normal/lost-reply CI at `69ba9de`, with no transfer credit or native top-up.
The strict empty-Cashu contact comparison passes the installed rehearsal at
`42e9956`. A later historical payout-8 upgrade failed because its pinned old
verifier cannot read core schema 26. The [compatibility correction](testing/cashu-upgrade-compatibility-20261117.json)
at `cb582e9` passes all ten selected-source workflows, including full native
gates and the installed systemd rehearsal. Its distinct reviewed empty-Cashu
source selector retains the default legacy requirement for both original and
selected verifiers. All 26 protected orchestration regressions also pass locally
without skips. The [two-configured-mint HTTP/financial composition](testing/cashu-two-mint-settlement-20261117.json)
now passes normal/lost replies at `0f66d53`, with all ten selected-source workflows
successful. Two original Strike receives deposit 396 sats before one synthetic
feeding, with four sats of conversion fees, zero remaining custody and no
transfer credit. The matched source/CI trees and artifact hashes are retained in
that checkpoint. Installed Runtime, Android, live provider and physical acceptance
remain open. The separate [native backup-copy helper](testing/cashu-copy-normalization-local-20261117.json)
passes local main/WAL, row-ID/signed-byte and 21 refusal/interruption checks,
strict Clippy and its CLI privacy test. The later
[FIFO correction](testing/cashu-copy-special-files-20261117.json) prevents blocking
before file-type validation and passes 24 refusal/interruption cases in isolated
root fixtures; its selected-source CI is pending.
The [runbook](deployment/cashu-backup-collection.md) keeps writer quiescence,
coordinated encrypted collection and funded recovery explicitly unfinished.

Cashu operator controls update **2026-11-17**: the
[protected planning/reconciliation checkpoint](testing/cashu-operator-controls-20261117.json)
passes 650 core tests (12 ignored), 179 wallet tests (seven ignored), required
format/strict Clippy/dependency/vendor gates and both actual CLI fixtures under
unused UIDs in private namespaces. The [current-source financial matrix](testing/cashu-operator-composition-20261117.json)
passes all eight HTTP/receipt/treasury cases with health accounting. A bounded
root-reviewed core request can preview/retain custody reservations and replay its
original intent. A separately approved wallet command can verify owned outputs
for one original acquiring operation without swap/melt replay or receipt export.
The [runbook](deployment/cashu-operator-controls.md) records exact scopes and
fixture limits: empty/unfunded CLI state and mocked systemctl remain separate
from funded native tests and installed acceptance. Held-policy administration,
treasury reconciliation, coordinated funded restore, multi-mint/economical-change
composition and independent client/provider acceptance remain open. No Cashu
service, operator seed installation or live financial/physical activation occurred.

Cashu private health update **2026-11-17**: the
[operational diagnostic checkpoint](testing/cashu-private-health-20261117.json)
passes 175 wallet tests (seven ignored), strict Clippy/format/vendor/dependency
gates and eight full HTTP/receipt/treasury composition cases with independent
health-accounting assertions. Private status now reports journal custody and
fees, queue age/lag, unknown legacy ages, future timestamps, retained core epochs
and fenced wallet registration mismatches. It reads no keys or provider state;
journal values are not current balances or spend permission. Constrained
operational commands, complete coordinated recovery and installed acceptance
remain open. No new runtime identity, key, service or financial/physical activation
was performed by this increment.

Cashu intake update **2026-11-17**: durable
[backpressure](testing/cashu-intake-backpressure-20261117.json) bounds uncredited
work and replay aliases. [Independent receives after compensated rejection](testing/cashu-mint-progress-20261117.json)
retain original recovery material and counter fences; all 170 wallet tests pass
(seven ignored). The [HTTP retry checkpoint](testing/cashu-http-retries-20261117.json)
passes all 648 core tests (12 ignored), strict Clippy and required local gates;
its separate-UID fixture retains two source records after 80 identical retries
per payment and preserves status across paused restart. The
[notice inventory correction](testing/cashu-license-authors-20261117.json) passes
13 tests and reduces missing notices to 15, with distribution readiness false.
The [current-artifact financial matrix](testing/cashu-final-composition-20261117.json)
now passes eight full HTTP/receipt/treasury IPC cases, including normal/lost
replies and repeated descriptor use, with actual CDK and an offline SDK sender.
The [dedicated seed preparation tool](deployment/cashu-seed-provisioning.md)
also passes eight public/synthetic tests. The operator confirmed seed staging;
metadata-only observation found a private 64-byte file under a 0700 directory.
The phrase was not supplied to Codex; runtime installation remains pending. Multi-mint/economical-change financial composition,
operational/recovery work and independent-client acceptance remain open in the
[full-plan map](testing/cashu-acceptance-gaps.md). The operator canceled the
security review; no completed-review claim or Cashu activation follows.

Cashu recovery update **2026-11-16**: the
[initialized combined native backup rehearsal](testing/cashu-empty-combined-native-backup-20261116.json)
now verifies 14 Cashu components/two mint stores together with all four native
CyberHerd journals and identical core bytes, and rejects Cashu/payout signer
substitution. The [standard core gate](testing/cashu-correlation-standard-gates-20261116.json)
passes 648 tests (12 ignored), strict Clippy and required local checks. The [corrected root correlation fixture](testing/cashu-correlation-root-result-20261116.json)
passes without weakening immutable bindings, and its
[full corrected-source gate](testing/cashu-correlation-corrected-gates-20261116.json)
passes all 648 tests and required local checks. Funded rollback, full key/client recovery,
writer-quiescent collection and external reconciliation remain open. No Cashu
service, schema, credential, payment or feeder activation is claimed.

Cashu source checkpoint **2026-11-16** remains an implementation in an isolated
branch under the [Nut November plan](planning/cashu-nutnovember-implementation-plan.md).
No Cashu service, schema upgrade, receiving, spending, public announcement or
physical activation is deployed by this work. The
[wallet/core boundary](architecture/cashu-wallet-boundary.md),
[compatibility checkpoint](testing/cashu-compatibility.md) and
[development runtime](deployment/cashu-wallet-development.md) distinguish source
fixtures from installed acceptance.

The 2026-11-16 selected-artifact qualification passes the full loopback HTTP,
authenticated receipt and treasury IPC, actual CDK and independently mocked Strike
composition. The [four-case SDK matrix](testing/cashu-repeated-request-result-20261116.json)
includes two distinct acquired ordinary payments to one descriptor, each with its
own capability, replay after credit consumption, and normal/lost melt replies.
Original credit remains stable and fee/source accounting is explicit. The wallet
passes 166 native tests with seven ignored plus strict Clippy/format/vendor gates.
That SDK checkpoint compiled its core fixture and ran protocol/vendor/audit checks.
The later recovery checkpoint above passes the full standard core suite; the
corrected recovery source now passes its separate full gate above. Independent end-user
applications, actual browser/phone/TLS, multi-mint settlement composition and
approved live/physical acceptance remain open.

The [native empty-state recovery fixture](testing/cashu-native-recovery-fences-20261116.json)
passes all three explicit core/wallet/Cashu-broker fences. The [offline inventory
verifier](testing/cashu-backup-foreign-keys-20261116.json) passes 18 tests and native
schema checks, including streaming memory bounds and foreign-key validation.
Funded rollback and a full combined inventory including the existing CyberHerd
membership, payout and payout-signer journals remain incomplete. No restore or
restart authorization follows from these checks. See the full
[acceptance map](testing/cashu-acceptance-gaps.md) for remaining requirements.

The separate wallet runtime implements protected startup, common acquired-proof
accounting, receipt retries, default-off NIP-61 discovery, and bounded publication
with exact signed bytes and per-relay acknowledgment. The purpose-limited signing
broker and nak adapter have synthetic NIP-46/NIP-44 and separate-UID IPC coverage.
Original-operation treasury admission, whole-proof selection, separately gated
splitting, paid recovery, combined fee/change accounting and retryable CDK cleanup
are wired. Cleanup preserves unrelated operations and permits subsequent same-mint
receiving. A missing unsubmitted saga can be reconstructed only when its original
quote UUID and complete proof reservations are retained. The earlier cleanup
snapshot passed **154 tests**, strict Clippy, formatting and vendor checks; its
source hashes remain unchanged. See the retained
[cleanup](testing/cashu-cdk-cleanup-20261116.json),
[quote adoption](testing/cashu-quote-adoption-20261116.json) and
[partial preparation](testing/cashu-partial-preparation-20261116.json) evidence.
Interruptions before proof reservation or before an original UUID is retained
still remain held and require further recovery work.

The core imports authenticated receipts once, preserves cap/retirement provenance,
and independently verifies purpose-bound Strike treasury receives without issuing
new donation credit. Financial feed plans now freeze source allocations before
physical dispatch, persist across restart and protect reserved credit from
retirement. Explicit local release retains history; a settled replacement plan is
linked to the superseded waiting plan. Default-off job planning creates bounded
per-mint treasury reservations atomically with their plan links, subtracting
verified coverage and pending transfers. Existing transfer jobs survive plan
release. Physical requests and confirmed debits use the exact financial parts;
unknown physical outcomes keep the original authority and UUID.

The [financial planning checkpoint](testing/cashu-financial-jobs-20261116.json)
passes **629 core tests**, with nine explicitly ignored fixtures, plus formatting,
strict Clippy, SQLite vendor verification, whitespace and a cached advisory audit
(1,509 advisories; 317 dependencies). Native default/all-feature dependency trees
contain no active RSA dependency. The initial retirement-test failure is retained
in the evidence; adding an explicit financial-release step resolves it. This is
working-tree source qualification, not release approval or installed acceptance.

The optional Cashu nginx include passes [four isolated edge tests](testing/cashu-nginx-20261116.json) for exact routes, method/body bounds and local log suppression. Installed WAF/CDN and sender acceptance remain open.

Independent [Cashu-TS 4.7.2 parser testing](testing/cashu-ts-request-initial-20261116.json) found a concrete compatibility blocker: both CDK-generated amountless `creqA` fixtures contain CBOR `a:null` and are rejected by that parser; both `creqB` fixtures parse correctly. This is isolated library-level evidence, not sender/payment qualification. A [standalone candidate correction](testing/cashu-ts-request-candidate-20261116.json) passes exact typed CDK round-trip, all four Cashu-TS parser cases and focused browser decoding/collision checks. The wallet encoder correction and separate-UID fixture children are now integrated; fresh core/wallet gates and a corrected CLI build are running. The [corrected native CLI output](testing/cashu-request-corrected-native-20261116.json) now passes both native fixtures, all four independent Cashu-TS parsing cases and all 44 browser tests. Integrated Rust gates and explicit separate-UID metadata composition remain running/pending; actual sender applications and phone scans are still unqualified.

The [two-QR frontend checkpoint](testing/cashu-overlay-20261116.md) passes all 44 website browser tests, including exact native-CDK request decoding at 1080p and 720p and full-roster/goat-card/banner/payment/feed-effect collision checks. Protected wallet selection passes its separate configuration checkpoint; read-only metadata IPC, public request HTTP and snapshot wiring are now implemented and under full qualification. Positive cross-UID metadata composition and real sender/phone scans remain pending.

Age/balance/operator sweeps now pass the [635-test core checkpoint](testing/cashu-sweeps-20261116.json), with nine explicitly ignored fixture entry points, strict Clippy, formatting, vendor checks and the cached advisory audit. This includes shared reservations when sweeps race feed funding, atomic operator requests and fair mint admission. The [combined wallet/core checkpoint](testing/cashu-combined-settlement-20261116.json) now qualifies the zero-fee 600/250/150 case with actual CDK acquisition/melt and independent mocked Strike verification; 636 core tests pass with ten fixture entry points explicitly ignored by the ordinary suite. The explicit combined runner passes separately. The [explicit combined lost-melt-response/early-Strike/paused-recovery fixture](testing/cashu-combined-lost-reply-20261116.json) also passes with source-matched native artifacts; all wallet and core gates pass with unchanged source hashes (155 wallet tests, 636 core tests; six and ten explicitly ignored fixture entry points respectively). The runner rejects older artifacts lacking its case markers. Combined fee/change and complete private-transport coverage remain open. Both inactive service examples pass syntax verification only; installed runtime/network acceptance remains open. Other
unfinished work includes protected operator release tooling, complete automatically
planned wallet-to-core settlement qualification, public reusable descriptor serving
and independent-wallet acceptance, positive HTTP-to-wallet UID qualification,
coordinated wallet/core/broker backup and restore, private custody/lag health,
the requested two-QR overlay and real scan matrix, inactive deployment artifacts,
installed signer acceptance, exact-commit CI/review and the November contribution
package. Cashu-derived rewards remain disabled. Production activation and real
payments or physical acceptance still require the explicitly approved operator
steps in the plan. Existing live checkpoint descriptions below remain unchanged.

CyberHerd membership audit **2026-11-16**: the
[morning join investigation](testing/cyberherd-membership-audit-20261116.md)
identifies a missing profile lookup on the actor's signed outbox relays,
shared core-bridge admission starvation and a stalled membership signer relay
bundle. Source corrections add bounded profile fallback, independent role
quotas/connection slots, shared payout-client pacing and individual signer-relay
fallback, alongside the pending optional-social liveness correction below.
The exact candidate passed all 14 CI checks and was installed with operator
approval on 2026-11-16. Fresh four-journal backup/native comparison passed;
Jr’s and nostrpoorgirl’s memberships recovered, three messages received relay
acknowledgements, and the pending input queue drained. This deployment evidence
is for the CyberHerd candidate, not the uninstalled Cashu work. Core’s missing
Strike payment-read permission was subsequently corrected by the operator;
normal reconciliation recorded 93 sats spent, 1 sat released and zero held.

CyberHerd intake checkpoint **2026-11-15**: the optional-social
[liveness correction](testing/cyberherd-intake-liveness-20261115.md) is under source
qualification. It parks unavailable social eligibility and audits invalid social
heads without holding later paid/feeding work. This correction is not installed;
existing live financial limits and source identities remain unchanged.

Outgoing reward checkpoint **2026-11-15**: the operator-approved
[zap-preferred ordinary-Lightning fallback](architecture/cyberherd-outgoing-invoices.md)
is [deployed](testing/payout-fallback-live-20261115.md), including receiver-supported
`CyberHerd Treats!` comments. The isolated signer's Primal-only connection repair
restored signing, and the retained provider journal records the first completed
reward (92 sats plus 1 sat fee). The native checkpoint reader now correlates
unpromoted fee-probe signatures; a fresh four-journal snapshot and verification
passed before all six programs were installed. This is current-state retention,
not external recovery adoption or completion of the full CyberHerd plan.

Public messaging checkpoint **2026-11-15**: signed [recipient relay routing](architecture/recipient-relay-routing.md)
is [deployed and qualified](testing/recipient-relay-routing-live-20261115.md).
New public interaction notes use the author’s NIP-65 write relays and mentioned
users’ NIP-65 read relays alongside configured direct relays and Send It.
Private NIP-17 inbox routing remains separate; downstream visibility is not measured.
The [lifecycle opt-out](testing/cyberherd-lifecycle-optout-20261115.md) and
CyberHerd stream-chat opt-out remain effective. This publication-only rollout does
not resolve the recovery-adoption blocker or complete the full CyberHerd plan.

Current CyberHerd checkpoint: **2026-11-14**. The full
[twelve-part implementation plan](planning/cyberherd-implementation-plan.md)
remains incomplete. It supersedes older CyberHerd deferrals; it does not resume
Monero or authorize financial/physical activation.

Current live state: the separately approved
[future-only reward cutoff](testing/cyberherd-initial-reward-cutover-live-20261114.md)
and [financial activation](testing/cyberherd-financial-activation-live-20261114.md)
are applied. All four services are running with the configured financial limits.
The [public messaging repair](testing/cyberherd-discovery-repair-live-20261114.md)
is deployed and merged, with automatic note replies independently verified.
The subsequent [stream-chat opt-out](testing/cyberherd-no-stream-chat-20261114.md)
is also deployed: CyberHerd notes and overlays remain enabled, while new and
retained queued chat are excluded from publication. The three-user roster recovered through normal relay backfill.
Older checkpoint descriptions below retain their stage-specific limitations;
they do not supersede these later live records. Full provider payout and recovery
acceptance remain separate from public-message delivery.

The operator supplied the financial limits and clarified that actual Strike fees
are deducted from member payouts. The [inclusive-fee integration](architecture/cyberherd-inclusive-fees.md)
is implemented, including fee probes and same-ID promotion into the guarded
execution chain. The [November 13 qualification and staged review](testing/cyberherd-inclusive-upgrade-checkpoint-20261113.md)
records all ten passing workflows for the selected artifacts, native/root and
installed upgrade comparisons, and operator-approved actual-host staging.
Staging checked all three retained worker configurations under their original
UIDs and sealed six programs without opening journals or changing services.
The separately approved [live additive payout-schema upgrade](testing/cyberherd-inclusive-upgrade-live-20261114.md)
now passes fresh native backup, clone/live comparison, exact installed hashes and
core/member resumption. Configuration is unchanged; outgoing roles remain masked
and financial limits remain disabled. Provider/recipient execution,
external recovery adoption and public CyberHerd rollout are not complete.
The separately approved [actual financial candidate](testing/cyberherd-financial-candidate-live-staging-20261114.md)
now passes native four-role validation with those limits, without installing
configuration or enabling payouts. The operator selected future confirmed
feedings only. Historical reward opportunities, including deferred feeding inputs,
need an audited cutoff before activation; that accounting change is not yet applied.
The [full acceptance inventory](testing/cyberherd-acceptance-matrix.md) tracks all
twelve requirements; a passing migration is not full-port acceptance.

The [per-payment feeder correction](deployment/per-payment-feeder-cap.md) is
already deployed: at most three feedings and one feeding announcement per payment.
The original donation's erroneous remaining backlog was retired once, preserving
its receipt and actual feeding history. Do not repeat that correction or erase
subsequent credits as part of CyberHerd maintenance.

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
all 17 original files/PIDs and zero spending limits remained exact at that
checkpoint. The operator later supplied the limits and inclusive-fee policy;
those inputs are no longer missing. No financial activation is implied by the
candidate or the subsequently prepared schema-upgrade review.

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
That earlier preparation is now stale after the approved three-seat/operator
configuration change and requires a fresh baseline before application.
The subsequent [freshly approved live upgrade](testing/cyberherd-program-upgrade-live-20261111.md)
has now completed against that current baseline, including native backup/review
and independent running-program/public-health verification. Original configuration,
three-user capacity and masked outgoing roles are retained.

The [first financial configuration transaction](deployment/cyberherd-financial-maintenance.md)
is now implemented separately in source. It consumes the sealed candidate and
completed selected-program review, takes a fresh stopped-writer backup/review,
refuses unreceived financial events or historical funded work, and separates
configuration application from explicit four-role activation. A native review
distinguishes payment/feeding cursor tails from ordinary overlay lag. Source,
protected-file and disposable installed qualification now pass the
[527-test source checkpoint](testing/cyberherd-financial-maintenance-checkpoint-20261111.md),
including all 154 protected-root upgrade cases, 41 browser cases and the exact
positive four-role systemd report. No actual-host financial transaction or
provider execution is established.

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

The [Nut November source inventory](testing/cashu-november-inventory.md) records the pre-November reference, Cashu starting revision, evidence scope and unfinished submission requirements. No final Cashu release/tag or submission is claimed.

The read-only metadata wallet increment passes [156 tests and all wallet gates](testing/cashu-public-metadata-wallet-20261116.json). The separate fee/change composition draft is prepared but uncompiled; its [runner rejects older zero-fee artifacts](testing/cashu-combined-fee-old-artifact-rejection-20261116.json). Positive fee/change qualification remains open.

The fee/change draft now has a passing [standalone synthetic mint-change probe](testing/cashu-fee-change-mint-probe-20261116.json) for real signatures/DLEQ, preserved recovery material and replay rejection. Actual CDK scheduler/change handling and combined accounting remain unqualified.

The [full-plan acceptance map](testing/cashu-acceptance-gaps.md) keeps all receiving, recovery, operational, interoperability and release requirements in scope. It additionally records the missing asynchronous sender status capability and protected financial-plan release CLI; neither is covered by the passing metadata/parser fixtures.

A [four-test standalone sender-status prototype](testing/cashu-receive-status-prototype-20261116.json) now verifies opaque capability persistence, nonce/epoch binding and evidence-bound status projection. It is not integrated into the wallet listener or public HTTP path; that requirement remains open.

Sender-status private dispatch/core forwarding/HTTP code is now drafted but uncompiled. The inactive status-route nginx addition passes [four expanded local edge tests](testing/cashu-status-nginx-20261116.json), including the 512-byte bound and capability log suppression. No application status endpoint or live activation is claimed.

The [request/metadata acceptance checkpoint](testing/cashu-request-acceptance-20261116.json) now passes full integrated Rust gates, positive separate-UID HTTP/WebSocket metadata (including rogue peer and paused restart), ordinary/lost-reply financial composition, corrected native CLI/parser/browser checks, and post-regeneration shared protocol checks. Canonical QR fixtures are now regenerated from the corrected binary. The first explicit metadata attempt failed because fresh-netns loopback was down; its setup correction and original failure are [recorded](testing/cashu-public-request-loopback-initial-20261116.json).

The next 15-file source increment is now integrated and under fresh core/wallet gates: immutable sender-status capabilities with private listener/core/HTTP dispatch, the bounded status route, protected exact-plan/epoch financial release with preview and stopped-core approval gates, and the real signed-change/nonzero-fee combined fixture. Their source is implemented, but application/root/positive fee/status transport qualification remains pending. No production services or financial settings changed.

### Cashu integrated development checkpoint — 2026-11-16

The uncommitted Cashu source now preserves the separately installed CyberHerd
membership/payout fixes. This checkpoint does not install or enable Cashu.

Wallet formatting, strict Clippy, 154 library tests, seven other native tests and
vendor checks pass; seven specialized tests are explicitly ignored by Cargo.
The core's isolated loopback rerun passes formatting, strict Clippy and 505
library tests, with ten ignored; all binary/integration gates, receipt protocol
checks, vendor verification and audit now pass, with source hashes unchanged
([evidence](testing/cashu-integrated-core-result-20261116.json)). The initial restricted-sandbox failure is retained separately and is
not used as a passing test result.

Matched native artifacts pass separate-UID HTTP admission/status, receipt,
signer and treasury checks, all six synthetic settlement variants, three root
financial-approval checks and eight root maintenance fixtures. The HTTP check
admits two pending payments without claiming acquired funds. The fee-only
settlement variants consume the original 1000-sat credit once without another
donation. Signed-change variants retain the one-sat dust and use later native
funding. Providers and confirmation are synthetic; no real payment or feeding
was performed.

The offline backup verifier passes twelve synthetic checks covering private
inventory, registrations, receipt/acknowledgment agreement and retained treasury
reports. Full coordinated restore, external outcome/ownership reconciliation,
complete financial IPC, multi-mint composition, independent senders, phone scans,
guarded release CLI, final public source/CI/package and operator-directed live
acceptance remain open. The [acceptance map](testing/cashu-acceptance-gaps.md)
links exact evidence and remaining requirements.
