# New VPS agent handoff: Phase 1 remediation

Operator request: finish deployment/packaging/operations corrections first, push
them for review, and continue remediation on the new VPS. Production is on HOLD.
This plan is for isolated development/staging and preparation of reviewable
changes. It is not production deployment or physical-actuation authorization.

Continuation evidence: `../testing/vps-remediation-20260910.md`. The new VPS
branch starts at the exact handoff SHA and adds F02 shared-store admission
regressions/correction. Keep remaining findings and production acceptance open;
consult the continuation record for actual checks and unresolved access needs.

## 1. Start from the reviewed integration branch

Read `AGENTS.md`, `audit-remediation.md`, `deployment-artifacts.md`, then the
canonical architecture and verification matrix. Earlier completion claims are
superseded by the 2026-09-08 audit. The recovery ZIP contained only an empty
candidate status record; there are no recovered patches to apply.

```sh
git clone https://github.com/lightning-goats/lightning-goats.git
cd lightning-goats
git fetch origin remediation/deployment-packaging-20260910
git switch --create vps/phase1-remediation --no-track origin/remediation/deployment-packaging-20260910
git rev-parse HEAD
```

Record that full SHA and the integration PR check URLs in the staging evidence.
If the integration PR has since merged, start from its exact reviewed merge
commit instead. Do not assume default-branch checkout contains the full stack.
Do not merge or deploy solely because artifact checks are green.

This branch includes the gateway/weather/CLN-removal stack, corrected nginx
contexts and full site examples, complete three-binary archives/provenance,
artifact tests and release gates. Payment/physical-control Rust defects remain.

## 2. Establish the isolated work environment

Use the separate deployment account, with only operator-authorized provisioning
privileges. Record OS/architecture, Rust 1.88, nginx and OpenSSL versions. Install
build tools and Python 3.10+ under that authorization; run locked Rust format,
Clippy and tests, the Security workflow's audit gate, and artifact tests. Build
all release binaries and run the packaging smoke procedure in
`deployment-artifacts.md`. Record failures honestly before implementing fixes.

Initially bind daemon, gateway, mock Strike and harmless mock OpenHAB only to
loopback. Use separate fresh daemon/gateway SQLite stores and synthetic tokens.
Keep all OpenHAB physical commands and production credentials outside this test
environment. Preserve existing production DNS and the old hub.

Discover or obtain the new VPS public address, approved staging hostname and
inventory of assigned WireGuard peers before preparing networking. `10.8.0.1`
is the old hub, `10.8.0.6` is the trusted host, and `10.8.0.10` is the operator
laptop. Do not guess an unused address or reuse an existing private key. Remote
access, household firewall changes and live credential operations require their
own operator-authorized scope; prepare proposed changes first.

## 3. Fix the physical-control boundary before connecting it

Implement F02 and F04 first, then F03. Use narrow reviewable commits with tests.

- F02: make safety admission and durable capacity reservation one atomic store
  operation. Enforce one unresolved request globally across UUIDs, processes and
  restart. Preserve same-ID replay without command resend; expose an audited
  reconciliation path. Test concurrent distinct UUIDs, timeout followed by new
  UUID, restart pending, cap boundary and database failure. Count actual mock
  OpenHAB commands, not just rows.
- F04: obtain sanitized exact request/result fixtures from read-only inspection
  of the current OpenHAB owner under authorized access. Record what means receipt,
  pending, no-action rejection and completed actuation. Implement a typed JSON
  adapter with fixed approved fields and UUID correlation; reject contradictory
  aliases, wrong UUID/producer and receipt-only acknowledgements. Do not change
  the household owner to match guessed payloads. Without fixtures, keep physical
  binding blocked and continue the isolated work below.
- F03: implement correlated `not_dispatched`, `pending`, `confirmed` and ambiguous
  results. Retry only authoritative no-dispatch outcomes after cooldown; query
  the same durable UUID after submission without resending commands. Bound and
  align client/gateway/OpenHAB deadlines; fix the shipped 2s/5s canary timing.
  Exercise real daemon + real gateway + mock OpenHAB with shipped examples and
  prove `2340 -> 2 confirmed feeds -> 340`, 423/429, hourly rollover, genuine
  network timeout, late completion and confirmation crash recovery.

Do not remove the daemon's unresolved-attempt safety constraint to regain
availability. Availability recovery must retain physical ambiguity protection.

## 4. Make settlement recoverable and verify invoice contracts

- F05: authenticate bounded notifications, durably persist inbox work, then
  respond promptly. Add a bounded worker and startup/periodic reconciliation of
  outstanding issued requests with pagination, backoff and durable progress.
  Use the same idempotent settlement transaction for every path. Test missing,
  duplicated, delayed and exhausted deliveries, slow provider reads, 429/5xx,
  database failure, restart after inbox persistence and extended outage.
- F06: verify current Strike receive/P2P documentation and define target-currency
  settlement using authoritative credited amounts and issued-request identity.
  Preserve original currency evidence and explicit rounding. Do not invent a
  conversion rate or credit an unverified invoice amount. Keep live P2P support
  unaccepted until a separately authorized low-value account test passes.
- F10: decode and verify real BOLT11 fixtures for checksum/signature, network,
  amount, payment hash, description hash and expiry. Test mismatches and absent
  wrapper fields; mock wrapper correctness is insufficient.
- F14: use one canonical Lightning Address validator at configuration, issue
  persistence and settlement boundaries, including periods end-to-end.

Keep receive-only authority, atomic settlement/credit/events and conflict
rejection intact. Do not issue real invoices or pay them under this plan.

## 5. Complete resource controls and presentation correctness

Implement F08 application budgets/concurrency, bounded queues and responses,
streaming body/provider-response bounds, JSON/method limits, redirect policy and
WebSocket connection/message/idle limits. Test multiple client identities,
exhaustion, chunked oversized data, signed duplicates and log redaction. The new
nginx checks cover only the edge; they do not satisfy application defenses.

Implement F11 versioned last-sequence resume, bounded replay/deduplication and a
heartbeat within nginx's 75s timeout. Implement F12 real observation age/future/
regression checks and explicit temperature units. Keep weather/presentation
failure independent from accounting and informational messages overlay-only.

For #26, obtain authorized read-only access to the original website source and
assets on the old VPS. Import into `web/`; preserve stream and independent Nostr
chat/auth, remove retired payment/NIP-05/CyberHerd calls, and use the confirmed
public contact identity. Test the actual browser overlay, reconnect, payment UI
and static routes. Do not reconstruct missing source from screenshots.

## 6. Prepare production-equivalent staging evidence

Follow F09's home inventory/source-attribution guidance in `audit-remediation.md`.
The firewall example intentionally applies nothing. Prepare a complete reviewed
home-enforced policy; prove gateway reachability and negative OpenHAB/weather/
SSH/Postgres/LAN paths, IPv4/IPv6, forwarding/NAT and hub source-attribution cases
in an authorized staging topology. Preserve household access and console recovery.

On a clean installation of the final integrated commit, record the full combined
functional/security matrix, root-owned binaries/config, separate runtime users,
credential scopes and permissions, TLS/static-site/browser and signer/relay
behavior. Exercise backup/restore with issued requests, settlements, both stores'
unresolved UUIDs and signed outbox bytes intact, with no financial or physical
replay. Keep one physical owner and actuation disabled throughout.

## 7. Return a reviewable result and stop at production gates

Push remediation commits and PRs, append exact regression evidence to the affected
issues, and keep #6/#15/#16 open until acceptance is complete. Report for each F01
through F14: commit/test evidence, remaining unknowns, and any required operator
decision. Preserve prior evidence instead of replacing it with a completion claim.

Handoff evidence must include the final source SHA, check-run URLs, tool versions,
binary/archive hashes, configuration revision, test command results, sanitized
owner contract and topology evidence, restore results and browser/signer results.
Separate artifact checks, mocked integration, live read-only observations and
operator-authorized live tests in that record.

After all source and staging gates pass, request separate explicit approval for
a tiny real payment, controlled physical feeder test, and production DNS/WireGuard
cutover. Never infer those approvals from this handoff or green CI.


## Continuation: durable settlement recovery

The F05 candidate is stacked on F03 source
`8580babd03733a9679d4d5f2183ab9c673806241`. Webhook acknowledgement now requires
inbox persistence; a supervised worker retries authoritative settlement and
independently scans issued requests even when notifications are missing. Scan
progress and retries are durable; all paths use the existing atomic credit/event
transaction. Username persistence/settlement uses the canonical validator (F14).
See `../testing/vps-remediation-20260910.md` for evidence and limitations.
F06 currency policy, F10 real BOLT11 checks, F04 owner fixtures, remaining F08/F11/
F12 corrections and staging acceptance still require their separate work. Keep
PRs reviewable and parent acceptance gates open; production remains HOLD.


The next F06/F10 candidate starts from `cc23a4f2dfce7eaa8492f3ddf40a129e22bef309`.
It verifies signed mainnet BOLT11 fields and issuance expiry, and uses
BTC `amountCredited` for P2P with exact whole-satoshi/no-rounding policy.
See the Strike architecture correction and VPS evidence. Actual P2P/account
network acceptance and F04 owner fixtures remain separate live dependencies.


F08 continuation from `7f54df087a0eaba1df6873e5318cb4b6143f5b8c` adds shared durable
issuance admission, nonwaiting HTTP/status/WS slots, handler/upload deadlines,
streaming OpenHAB bounds and redirect rejection. The daily issuance budget is
10,000, and recovers as its 24-hour window advances without deleting issued evidence.
Operator capacity/archival planning remains required for historical storage. Settlement
recovery remains independent. See the VPS evidence; F11 replay/heartbeat, F12
weather correctness, owner fixtures and final staging gates remain outstanding.

F11/F12 continuation starts at `672763db6d36c696424e24fab92989a7c1b95f8a`
(draft #36, Rust/Security/Deployment all passed). It adds bounded versioned
WebSocket replay/heartbeat and actual weather age/units. Follow the explicit
protocol in `../architecture/overlay-stream.md`; browser acceptance awaits the
authoritative website. After offline ledger restore, before any daemon restarts,
run `lightning-goatsctl --config <restored-config> reset-overlay-stream` so events
reusing old sequence numbers cannot be silently skipped by old clients. This
only rotates presentation identity; never clear unresolved physical requests,
issued requests, settlements or signed outbox evidence to recover availability.
Keep all drafts pending integrated review and the remaining acceptance gates.

The staging-evidence candidate starts at F11/F12 source
`a3b17e809ba956407a98c35a07685f3d61402050` (draft #37, all checks passed).
Follow `staging-acceptance-evidence.md` for paired-store restore and the F09
inventory/dependency matrix. The restore helper creates a new private destination,
checks both stores and rotates only overlay identity; it never starts services
or overwrites current data. CI retains the exact tested package, labelled with
its actual build SHA, for staging review. Final installation, actual owner/source,
network, browser and signer acceptance remain open. Do not merge this stack just
because individual PR checks are green; preserve exact integrated review evidence.

Installed-release rehearsal continues from PR #38 head
`76407c0ba8821ee53b4ffcb044e5a6d9b6733537`. Read `install-rehearsal.md` before
running the temporary root-owned/non-root-process package test. It refuses host
network access and uses only synthetic file credentials and an internal mock
owner. CI retains its JSON evidence and hashes. Do not convert those results into
final systemd, live credential, browser, network or physical acceptance claims.

The installed-release rehearsal passed on Ubuntu CI and this Fedora VPS. Its
sanitized evidence is `../testing/evidence/installed-release-fedora-20260910.json`:
exact runtime source/archive/binary hashes, separate UID/GID, zero capabilities,
NoNewPrivileges, credential isolation, six discovery routes and one mock command
across duplicate/restart checks. No host user/service or route was created. The
harness disables bytecode writes and its temporary state/processes were removed.
Final systemd/LoadCredentialEncrypted, actual identities, privilege revocation and
all live acceptance gates remain outstanding; this is not cutover authorization.

## Actual systemd rehearsal continuation

The dedicated systemd branch starts from draft PR #39 head
`5becfbd4b8b321ecb205a70233d1970a822645f9`. PRs #31–#39 were rechecked:
all draft/open/unmerged, all current Rust/Security/Deployment checks successful,
and no submitted GitHub reviews. Preserve stack order and require integrated
review/checks before merging; merging does not lift production HOLD.

Read `systemd-rehearsal.md`. The continuation exercises the shipped canary sandbox
through actual transient system units, synthetic `LoadCredentialEncrypted`,
wrong-name/corrupt credential rejection and real-binary restart idempotency in a
new loopback-only namespace. No production credentials or host networking are used.
The staging host credential key was initialized root-owned mode 0400 after the
empty-store checks; it is retained securely and never included in evidence.

Fedora initially rejected outbound service connections because disposable `/run`
binaries retained a runtime-directory SELinux label. Matching their default
`/usr/local/bin` destination labels fixed the rehearsal while preserving enforcing
SELinux and all host policy. Record the default `unconfined_service_t` domain
accurately; this does not claim a custom SELinux application policy. Final
installation must restore/verify the actual destination labels.

Owner/site read-only access, staging address reservation/peer registration and a
staging hostname remain pending operator input. The maintenance record's pending
kernel reboot, final identities/privilege revocation and all live gates remain
open. Nothing in this rehearsal authorizes real payments, feeding or cutover.

Draft #40 initial commit `fffee25537e6e5194c381a1f96afe6d5b300ae32` passed all
three GitHub workflows, including actual systemd on Ubuntu. The follow-up repeats
the gateway startup probe while the daemon is live to establish both credential
isolation directions and records five starts with zero automatic restarts. The
Fedora record is `../testing/evidence/systemd-rehearsal-fedora-20260910.json`.
Read final #40 check results before accepting the follow-up; retain the earlier
CI/source evidence. Full local locked Rust and deployment tests passed, while
the dependency audit runs in GitHub because cargo-audit is not installed locally.

## Integrated review: durable SQLite startup

The review of #31–#40 found an inherited daemon durability defect: a `sqlite://`
prefix can still select a volatile database. The correction starts from final
#40 head `dc5a88c3371d62d2795b067df42bc005fe0de334`, whose Rust, Security and
Deployment workflows passed. Read `../testing/sqlite-durability-review.md` and
its preserved failing regressions before accepting the follow-up. Both stores
now share filesystem URL validation and effective WAL/FULL checks before schema
changes and on every pool connection. The gateway already rejected the tested
memory VFS cases at runtime; do not report those as a demonstrated bypass.

Review and recheck this correction before merging the stack. Keep the F02/F05
durable-state acceptance conditions and parent gates open until verified on the
final integration head. Actual owner/source access, effective network containment,
website/browser and final privilege/credential decisions remain outstanding.

### Corrected runtime installation evidence (2026-09-11)

The exact #41 source `a54ead65b7d68a402b2e4a33ff387b6198a9cf84` passed all three
GitHub workflows and the full local locked Rust suite (138 tests). A fresh local
optimized archive now passes the actual Fedora systemd/encrypted-credential
rehearsal as well; `systemd-rehearsal.md` links its source-pinned records. Five
starts, both live-peer credential-isolation directions, zero automatic restarts
and one total harmless owner command were observed. The packaged optimized CLI
also rejects the volatile SQLite regression while preserving ordinary file state.

Current read-only host evidence supersedes the earlier pending-reboot note:
kernel `7.2.4-200.fc44.x86_64` is running and DNF reports no reboot required.
No reboot was performed in this continuation. Recheck before final installation;
final identities, privilege revocation, SSH and credential gates remain open.
The existing owner/site access and staging identity/hostname dependencies remain;
website contact at that point additionally needed the operator-approved public
Nostr key (supplied in the operator decisions update below).
Production remains HOLD and all parent acceptance gates stay open.

### Persistent inactive staging installation (2026-09-11)

The next continuation starts from final draft #41 head
`d350b2af697b0add7056a353c2fcfe97c697b8ce`. PRs #31–#41 remain draft/open and
unmerged; #41's Rust, Security and Deployment checks passed. Read
`inactive-vps-canary.md` and its sanitized installation/readback evidence.

The fresh VPS now has a separate locked, non-login, no-sudo runtime identity;
root-owned daemon/CLI, canary example and inactive canary unit; and empty private
state. The installed runtime is corrected source `a54ead65`, not a final merged
release. No active configuration or canary credentials were installed, no service
was started/enabled, and no systemd reload, SSH or network change was performed.
The helper refuses existing installations; preserve this state on continuation.

The initial account-only failure, its verified unused-account cleanup, corrected
sudo-denial guard and successful repeat are documented without suppressing the
earlier failure. Final production installation, integration review, privileges,
actual owner/site access and live acceptance remain open. All production gates
remain on HOLD; merges do not authorize activation or cutover.

### Public domain readiness baseline (2026-09-11)

The DNS continuation starts from final draft #42 head
`361e28e1b6c7bee724c7e462f611bf841591fc41`; its three workflows passed,
including the corrected fresh inactive Ubuntu installation. Read
`domain-readiness.md` for public authoritative DNS/registry/TLS evidence and the
remaining #19 account/zone/CA/DNSSEC decisions. The apex still points to the old
VPS at `45.76.234.192`; `www` aliases it, both with TTL 3600. No DNS change was
made. This is an apex/`www` baseline, not a complete zone or account audit.

No child DNSKEY/parent DS or apex CAA answer was observed. Registry transfer/update
locks are present, but account MFA/recovery and provider eligibility remain
unverified. Preserve mail/TXT, the old certificate dependencies and all production
gates. Owner/site access, staging registration/hostname and final SSH/credential/
privilege decisions remain pending; keep #6/#15/#16/#19 open.

### Unapplied SSH hardening review (2026-09-11)

The SSH continuation starts from final #43 head
`109ddbed760f703992a1d3eef9ec3cbc22a4601a`, whose Rust/Security/Deployment
checks passed. Read `ssh-hardening-review.md` and its Fedora evidence before any
SSH change. The candidate early snippet passed actual syntax/effective-policy
checks, nine contexts and ordering/Match negative controls in private temporary
copies. The on-disk policy still permits passwords and root keys; nothing was
installed or reloaded. Public-key operator login and provider-console recovery
remain unverified and must be confirmed before the application step.

The read-only helper refuses unreviewed include graphs, Match blocks and service
options. Preserve the pinned original files and review drift; do not use the
candidate as an unconditional host bootstrap. Final account/key cleanup, privilege
revocation, owner/site access, staging registration and all live gates remain open.

### Operator decisions received (2026-09-11)

This continuation starts from final draft #44 head
`954fc3e1facfea2921e495bedaf6f46af40237d8`, still open and unmerged, with
Rust/Security/Deployment checks passed. No application code or host policy changes
are included in this decision record.

- Website contact: the operator supplied the public Nostr key now recorded and
  locally validated in `public-site-migration.md`. Identity selection is complete;
  authoritative source import and actual browser contact testing remain open.
- Domain: the operator controls the registrar and ZoneEdit accounts and approved
  Let's Encrypt only. `domain-readiness.md` contains the unapplied CAA proposal.
  Hardware-key MFA, tested account recovery and full-zone review remain pending.
- SSH: the operator reports fresh key logins for `sat` and `linuxuser` and working
  provider-console recovery. Retain `sat` and provision its approved public key
  first. The host recheck still found no authorized-key file for that account;
  key selection/provisioning and a fresh login with the installed key are pending.
  See `ssh-hardening-review.md`; no SSH configuration was installed or reloaded.

These updates supersede the unanswered identity/access questions above only to
the stated extent. They do not approve production DNS changes, establish actual
owner/network/site integration or alter payment/feeder authority. Keep the stack
draft, production HOLD and #6/#15/#16/#19 acceptance gates open.
