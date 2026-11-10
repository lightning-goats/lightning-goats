# Phase 1 Verification Matrix — Risk-Tiered Live Pilot

Effective 2026-09-14; tracker #15. Apply
[parallel-live-pilot.md](../deployment/parallel-live-pilot.md) first.

This replaces the former all-checks-before-first-payment launch gate. Keep existing
regression suites and historical audit evidence; do not represent skipped or
unperformed cases as passed. A concrete fault blocks the affected path. The complete
checklist is not a prerequisite to the operator's small parallel live pilot.

## A. Before first pilot payment: short preflight

| Check | Required result |
| --- | --- |
| Origin/TLS/routes | `herd@feeder.lightning-goats.com` resolves to the new daemon; callback/metadata use the pilot origin; old public addresses unchanged |
| Issuance and settlement | Valid signed mainnet BOLT11 binding amount/hash/expiry; authoritative Strike completion, not a trusted webhook body alone; amount/user restrictions retained |
| Secrets and runtime | Protected receive/read-only Strike key, no spend/OpenHAB key on VPS; non-admin runtime and existing service protections |
| State and provenance | Fresh pilot durable ledger on first start, selected source/binary/config recorded; no fixture/copied production ledger; no known corruption defect in this path |
| Ability to stop | Close new pilot issuance/dispatch without deleting state or disrupting old production; preserve late settlement recovery |

No fixed payment-count/220-sat/time ceiling was adopted. The operator chooses manual
payments within configured invoice and feeding limits. Sandbox provisioning, complete
weather/site work, optional guard tooling and extra agents are not prerequisites.

## B. Before and during live feeding: inspect the actual path

| Check | Required result |
| --- | --- |
| Gateway target | Intended existing local owner, compatible UUID/completion contract; VPS reaches only the narrow gateway, no generic OpenHAB/weather/admin route |
| Parallel arbitration | Both paths obey the same local owner/interval/cap; otherwise legacy feeder dispatch alone is paused and its credit retained |
| Local safety | Override/enable, minimum interval, absolute cap and persistent duplicate controls remain effective |
| Start state | Inspect accumulated credit/feeds due/unresolved UUID before enabling; no empty-state assumption after earlier real payments |
| Delivery and debit | One actual completion for the UUID produces one debit/event; receipt alone is not completion |
| Ambiguity/capacity | Unknown result blocks fresh automatic dispatch; reached retention capacity stops admission, never causes identity deletion/reset |

A quick initial shadow payment is optional. `canary` can perform real feeds without
public Nostr; `active` additionally publishes. Do not assume a mode name identifies
the physical versus harmless gateway target. A failed physical preflight need not
prevent clearly labelled payment-only testing with new dispatch disabled.

## C. Observe through operator payments, not another synthetic launch ceremony

Record one durable credit/event per real settled receive and correct `address_user`.
Observe threshold crossing, actual feed, correlated completion, debit and remainder.
At a 1000-sat threshold with zero starting credit, 2340 sats means two confirmations,
340 remaining and no third feed. Adapt the expected result to the real starting balance.

Restart only the pilot process against its unchanged database. Confirm no extra credit
and no repeated physical command for a confirmed UUID. Inspect same-ID replay/status
without inventing a fresh request. Stop on extra feeding, contradictory money or unknown
delivery. Do not inject dangerous failures into a live actuator to fill a checklist.

Preview overlay payment/feed messages and reconnect behavior. Weather may be unavailable;
when enabled it uses actual observation age/units and remains overlay-only. In canary
mode, public Nostr is intentionally untested/off. If active mode is enabled, observe
normal payment/feed posts and exact signed-event retry without a historical flood.

Genuine Strike webhook delivery is distinct from scanner recovery. The first payment
may use recovery scanning while subscription work is pending; report that limitation.
Later verify real signed notification handling/recovery without duplicate credit. Do
not count a synthetic HMAC payload as provider delivery or a Lightning-only payment as
proof of Strike-to-Strike P2P/FX behavior.

## D. Keep the automated regression inventory

Run existing relevant suites on the selected candidate; reuse matching CI evidence.
For Rust changes:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
```

Also use the existing Security/Deployment gates. Check actual exits/logs, not a green
`tee` pipeline or an empty test selection. Ordinary-suite ignored tests are not counted
as executed; distinguish dedicated-workflow evidence. No tests or workflow assertions
are removed or weakened by this planning revision.

Preserve coverage for:

- signed invoice signature/checksum/network/amount/metadata/expiry and invalid users;
- provider errors/429/timeouts, forged/missing/reordered notifications, durable recovery,
  conflicting IDs/hashes and atomic credit/event creation;
- SQLite file/WAL/FULL startup, concurrency, duplicate/restart persistence, interrupted
  intent and ambiguity-safe UUID recovery;
- feeder override/caps/refusal, real daemon-to-gateway with harmless-owner counts,
  and ordering such as receipt before debits (do not use the known-bad #82 checker
  as accepted evidence; #91 is its correction candidate, subject to normal review);
- constrained nginx/webhook/WS ingress, authentic peer containment including forwarding,
  established flows, IPv6 and rollback; separate synthetic namespace evidence from hosts;
- overlay replay/reset/heartbeat/limits, weather units/observation age/regression,
  exact signed Nostr retries, release/installation/credential-isolation checks.

These remain engineering tools, not requirements to re-run every experiment on both
real hosts before a manually observed pilot. Do not count source review as live evidence.

## E. Follow-up and operator cutover

Additional goat-address coverage, public webhook/P2P variants, extended browser/weather
compatibility, long-retention/co-restore redesign, optional CI tooling, comprehensive
hardening and full-host rehearsals remain follow-up unless an identified defect affects
the enabled pilot. Existing local capacity and secret restrictions still apply. Never
perform destructive restore/fault experiments against paid state just to complete a row.

The operator decides when live results justify the DNS/Nostr-profile switch. Use
[production-cutover.md](../deployment/production-cutover.md), preserving pilot state,
old outstanding invoices/credit and one physical dispatcher/owner. Hub migration need
not coincide with payment cutover. Check additional addresses as they become public;
there is no requirement for six paid tests or closing every parent issue first.

## Evidence format

A short source-pinned note with starting/ending credit, receive/feed counts, restart
result, concrete failures, enabled modes and still-untested features is enough. Keep
sensitive payment/account details private. Leave unverified items open without making
each one a launch gate. Previous exhaustive matrix/history is retained in Git at
`df6be90f1b0505d183643ec55a298b382cd43b6c` and in existing audit/testing records.

## Local operator credits — 2026-11-08

The operator-approved local grant path and per-event 10-sat announcement threshold
are specified in [operator-credits.md](../deployment/operator-credits.md). Validate
atomic/idempotent grants independently from provider settlement, plus concurrent
retry, exact feed debit through the existing override, 9/10-sat overlay and Nostr
boundaries, and preserved real paid state during deployment. Isolated grant/feeder
fixtures are not evidence of a live Lightning payment or physical feeding.

## Paused queue messages — 2026-11-09

Verify paused credit retention and confirmation-only debits, including an already
in-flight feeding completing after pause. Advisory message state must never
authorize dispatch or block financial credit. Cover queued versus pending counts,
restart/retry stability, normal information/weather while paused and their
nonpublic audience. The implementation/acceptance contract is in
[paused-feeding-queue.md](../deployment/paused-feeding-queue.md).

## Household NIP-46 and audience separation — 2026-11-09

Verify legacy shared-relay compatibility, independent bunker/publication routes,
client credentials only during signing, and identical persisted events on relay
retry. Validate the actual household identity/allowlist and a signing-only round
trip as the daemon UID before public activation. Weather/information must remain
overlay-only, including paused events; scrolling overlay text omits the paused
prefix while the banner remains. Current configuration/evidence is in
[household-nostr-bunker.md](../deployment/household-nostr-bunker.md). Do not treat
relay EOSE or a signature as proof of public note acceptance. The new audit
finding is recorded [separately](../security/sqlite-wal-advisory-20261109.md).

## CyberHerd payout outcome source regression additions

For `feat/cyberherd-20261109`, qualify the separate journal/core outcome protocol
using temporary databases and loopback provider credentials. Verify independent
core reads disagree safely with a claimed terminal state/cost; exact completed
costs and unused fees; failed-cost holds; lost final ACK/restart; denied intake
role; permanent never-executed closure; retained execute uncertainty; partial
hash-chain ordering and rollback on final-write failure. Run full locked Rust,
deployment and dependency-audit gates for the exact candidate source. These tests
do not authorize production spending or replace the full CyberHerd port contract.


## Optional CyberHerd lifecycle source checks

The standalone port's lifecycle checks are recorded in
[`cyberherd-lifecycle-checkpoint-20261110.md`](cyberherd-lifecycle-checkpoint-20261110.md).
Its ten new regressions cover actual-override/unknown handling, quiet initial OFF,
initial ON, ordered note/chat relay receipts, exact signed retries, invitation
cooldowns/cancellation/capacity, template opt-out, evening context, backpressure
and the fixed read-only gateway Astro route. The final code passed all required
Rust commands through source-pinned CI and inactive artifact checks. The known
unignored SQLite dependency advisory remains a release blocker.

Before opting into live lifecycle notices, still qualify the installed dedicated
UIDs/configuration, household gateway's three fixed Astro Items and unknown/error
behavior, the distinct household NIP-46 client for kinds 1 and 1311, and actual
relay acknowledgments. Review the retained outbox and baseline before activation;
do not clear signatures/cooldowns or infer permission for feeding/payments from
these read-only presentation tests. The full CyberHerd completion contract remains
in `docs/planning/cyberherd-implementation-plan.md`.


## Optional CyberHerd confirmed-feeding presentation checks

The [458-test source checkpoint](cyberherd-feeding-announcements-checkpoint-20261110.md)
records nine new regressions for original triggering-payment attribution, frozen
herd summary/stream context, per-payment presentation deduplication without loss
of per-UUID rewards, native event-ID uniqueness, exact retries, optional templates,
legacy operation, nonpublic/ambiguous states and correlated private overlay import.
The full source-pinned Rust and inactive artifact checks passed; the unignored
SQLite advisory remains open. These temporary-ledger fixtures are not physical
feeding, provider-spend or public Nostr acceptance evidence.

Keep `core_bridge.delegated_feeder_messages=false` until the compatible intake,
root-pinned owner/epoch/cursors and separate identities/signer have been qualified.
An inactive installation rehearsal must also prove that lost overlay ACKs and a
paused member queue cannot duplicate physical effects, food/debits or text, and
that new equal-valued payments remain distinct while same-payment batches share
one announcement. Public relay and household-client acceptance are separate from
the synthetic fixtures. Follow the full CyberHerd contract before activation.


## Optional read-only roster and repost-hint source checks

The [466-test checkpoint](cyberherd-roster-checkpoint-20261110.md) records the
completed exact-source Rust, 24 browser, nine CLI and inactive archive checks.
The existing SQLite audit remains open; new live roles/features are disabled.

Verify exact point/share projection and sat strings above the browser safe-integer
range, active/banned filtering and public-picture bounds. Exercise API pagination,
revision conflict, stale-cache refusal, unknown-vs-empty target discovery,
non-loopback configuration refusal and absence of mutation/private fields. Browser
fixtures must block all external traffic and show that default flags make no API
request, unsafe names/images stay inert, exact amounts survive and empty rosters
clear. Private IPC must reject wrong owner/epoch/role and changed stale revisions;
lost ACK returns one sequence, and the atomic reconnect snapshot retains current
cards without money, food effects or core Nostr jobs.

For optional no-treat hints verify both frozen/current repost awards, category
opt-out, once-per-cycle persistence, canonical source aliases and rollback of the
claim/cursor when message insertion fails. Preserve the full locked Rust checks,
audit gate, inactive CLI/archive and existing browser/systemd regressions. These
fixtures do not authorize installing the proxy/roles, signing publicly, spending
or feeding. Browser authentication/operator controls and full-port operational
qualification remain separate requirements.


## Optional Nostr HTTP sign-in and operator commands

Verify native NIP-98 signature/ID, exact configured HTTPS origin/path/query/method,
mandatory command body hash, time window, singleton tags, GET-body rejection and
replay prevention across reopen. User self reads must never become operator
reads through payload/Host/forwarded-header claims. Root configuration alone
pins operator public keys; templates and commands accept no private-key fields.

Verify atomic revision-fenced settings/member/template/reset changes, explicit
unban, zero member pool, exact desired totals, no fabricated lifetime payment,
once-only promotion of retained social types, retained receipts and owner scope.
Inject failure after state changes and before audit insertion: retry must retain
its identity with no partial mutation/nonce. Core epoch/UID/high-water/freshness
and pending-feeding fences must prevent editing an already-confirmed feeding's
snapshot. Known committed retries recover without a fresh core ordering read.
Manual notices freeze their own human/public bodies and one optional compact share
footer; no reset announcement, fake received payment or additional mention is made.

All browser traffic is mocked. Check disabled flags make no API/signing request,
nonoperators cannot reveal controls, native browser signing binds exact bodies,
and a lost response retains one command body/UUID while explicit retry signs a
fresh HTTP event. No automatic mutation retry or raw/private-key login is allowed.
Keep full locked Rust, audit, inactive CLI/archive/browser/nginx/systemd gates;
actual new household client, WAF, UIDs and activation remain separate evidence.
