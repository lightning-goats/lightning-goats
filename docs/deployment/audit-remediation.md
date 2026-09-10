# Phase 1 audit remediation record

Decision: **HOLD production deployment.** Audit date: 2026-09-08. This correction
was prepared on 2026-09-10 for deployment/packaging/operations. Earlier completion
evidence is retained; it is not production acceptance.

## Recovery and source identity

The supplied `lightning-goats-remediation-recovery.zip` contained only
`recovery-status.json`, recorded at `2026-09-08T17:25:48.783777+00:00`, with
`candidates: []` and `publication: "not verified by this recovery script"`.
It contained no patch, source, nginx reproduction or behavioral-probe files.
Do not claim those files were recovered or independently rerun here.

A fresh retrieval confirmed `main` at
`80ae37c7b20950b345a1510fa16a05708925f8b8`. The full stack is available at
`5347733c5fbcde9b57dede3ee5aee33c0c20532b` on `phase1/template-messaging`.
The `phase1/audit-remediation` tip `d2e8f1617cbd0711b1e883f6954a1267efef8e9a`
adds a read-only workbench workflow, not source corrections. These observations
are from retrieval time; check current refs before integration.

## Finding disposition at the integration handoff

Verification for this work package (2026-09-10): all 11 new deployment tests
passed in a disposable nginx 1.26.3 container with the repository mounted
read-only. Five tests exercise both assembled TLS sites against loopback mocks;
six test archive completeness, corruption, source mismatch and unsafe members
using harmless executable fixtures. `bash -n` passed for the packaging script
and `git diff --check` passed. No production services or network policy changed.
The laptop has no Rust toolchain: real-binary build/package and locked Rust/
Security results must come from the integration PR's GitHub Actions runs.

| Finding | This work package | Remaining gate |
| --- | --- | --- |
| F01 | Full stack integrated with current main on review branch | Explicit integration PR review/merge; checks on exact final commit |
| F07 | Split contexts, quote regexes, full TLS site examples and isolated routing tests | Production-equivalent TLS/static-site/IPv6 installation |
| F13 | Package/hash/smoke-test three binaries, examples and provenance; release requires integrated source and checks | Build/package evidence on final merge result |
| F09 | Non-applying firewall worksheet; corrected source-attribution guidance | Actual home inventory and approved topology tests; no containment claim |
| F02, F03, F04 | Rust source unchanged | Durable global admission; typed refusal/polling/deadlines; exact owner adapter |
| F05, F06, F10 | Rust source unchanged | Durable webhook/recovery worker, P2P policy, BOLT11 verification |
| F08 | Some edge method/body/content-type checks covered | Application budgets/queues, streaming bounds, redirects and WebSocket limits |
| F11, F12, F14 | Rust source unchanged | Overlay replay/heartbeat, weather age/units, canonical username validation |

Keep #6 and verification/cutover gates #15/#16 open. Add targeted remediation
follow-ups to affected implementation issues while preserving earlier evidence.
This record does not change issue states or certify tests that have not run.

### New VPS follow-up

See `../testing/vps-remediation-20260910.md` for the source-pinned continuation.
F02 now reserves shared-store capacity atomically and preserves unresolved UUIDs
across processes/restarts. The new real-gateway process regressions reproduced
two OpenHAB mock commands on the handoff code and pass after the correction.
F03/F04 remain open; this does not accept the current physical owner binding.
The deployment test harness also isolates Fedora nginx's default temporary paths.

F03 follow-up adds correlated outcomes, immutable no-dispatch tombstones,
restart-safe cooldown and GET-only recovery of unresolved attempts. Shipped
canary timing is now 5s/5s. Real daemon → real gateway → harmless mock OpenHAB
tests establish two commands/two confirmations/340 sats from 2340 synthetic
sats, plus response loss, restart and confirmation-write failure recovery.
F04's live owner binding and complete staging/physical acceptance remain open.

## Home network acceptance (F09)

Inventory the trusted home host before proposing changes: WireGuard interface,
peer public keys and per-peer `AllowedIPs` (no private keys); UFW numbered/raw
rules; nftables/iptables IPv4/IPv6 rules and defaults; forwarding/NAT; listeners;
systemd IP restrictions. Keep private topology evidence operator-controlled.

The laptop's `10.8.0.10` address is not itself an authenticated administrative
identity through a compromised hub. If the home peer accepts the entire VPN
prefix from the VPS key, that key can carry traffic claiming any source in the
prefix. Home policy must contain all traffic attributable to the untrusted hub,
including forwarded/source-attribution cases. See
[WireGuard cryptokey routing](https://www.wireguard.com/#cryptokey-routing).

A narrow UFW allow does not remove a broad earlier allow. Review rule order,
established connections and alternate trusted paths. A second interface on the
VPS is insufficient while its broad path remains. Preserve independent end-to-end
authenticated administration and console recovery. Gateway systemd's subnet-wide
allowance is not peer authentication.

Prepare the complete home-enforced policy from actual inventory, IPv4/IPv6 and
routed/NAT paths. Rehearse in isolation before an operator reviews household
network changes. Test authorized staging positively to the harmless gateway and
negatively to OpenHAB REST/admin, weather :5000, SSH, PostgreSQL, unrelated LAN
and VPN peers. Include forwarded/source-attribution cases from the untrusted
hub's accepted prefix; preserve legitimate household/admin traffic and recovery.

## Acceptance before lifting HOLD

Run the exact integrated tree on a clean isolated installation. Real daemon and
gateway modules must use harmless OpenHAB and mocked/sandboxed Strike. Cover
concurrent/deduplicated UUIDs, pending restart, no-dispatch 423/429 cooldown/hour
rollover, network timeout, late acknowledgement and confirmation crash. Assert
OpenHAB command counts and `2340 -> 2 confirmed feeds -> 340` with corrected
shipped timing. The original 2s/5s timing defect is corrected by the F03 follow-up above;
artifact checks alone do not establish the physical owner contract.

Settlement tests must include missed/duplicate/late notifications, durable inbox
restart, database unavailability, provider 429/5xx and extended outage, asserting
eventual single credit. Add exact sanitized owner fixtures, supported P2P currency
cases and valid BOLT11 fixtures.

Then record credential scopes, trusted networking, TLS/static-site/browser
overlay, signer/relay, package provenance and backup/restore without financial or
physical replay. Keep one physical owner and production actuation disabled. Only
after all gates pass may an operator separately approve tiny payment, controlled
feed and production cutover.


### Settlement recovery follow-up

F05 now has a durable authenticated webhook inbox and a supervised independent
recovery worker over all locally issued requests, with bounded pagination,
persisted progress/backoff and atomic idempotent settlement through the existing
ledger transaction. F14 uses the canonical username validator at issuance and
settlement. See the source-pinned VPS evidence for focused regression results.
F06/F10 and end-to-end live settlement acceptance remain open; mocked provider
wrappers do not verify invoices or authorize payment tests.


Current VPS source disposition (supersedes the historical source-unchanged rows):

| Finding | Reviewable correction | Remaining acceptance |
| --- | --- | --- |
| F02 | PR #32 atomic shared-store admission; command-count regressions | Final integrated checks and actual owner binding |
| F03 | PR #33 correlated polling/refusals/cooldown; shipped timing | F04 contract and final staging matrix |
| F05 | Settlement follow-up durable inbox, fair scans, retry/quarantine | Exact-head checks, production-equivalent restore/settlement acceptance |
| F14 | Canonical validator at issuance, persistence and settlement | Final integrated review |
| F04 | No acceptance claimed | Exact physical-owner contract and sanitized fixtures |
| F06, F10 | Signed invoice and authoritative credited-BTC correction with synthetic fixtures | Exact-head checks, live P2P/account network acceptance |
| F08, F11, F12 | Provider/webhook body bounds partly implemented | Global resource controls, overlay protocol, actual weather freshness/units |
| F01, F07, F09, F13 | Earlier integration/artifact work retained | Integration review, final package, clean install, home containment and website acceptance |


F06/F10 follow-up: actual signed mainnet BOLT11 verification now binds network,
amount, hashes and issuance expiry; recovery permits expired but valid issued
invoices. P2P requires authoritative BTC credited amounts and preserves original
currency evidence with a no-rounding policy. Mock evidence is recorded in the
VPS report; actual account behavior and sandbox network compatibility remain
unaccepted. No real payment or physical test is implied.


F08 continuation adds shared durable global issuance admission, bounded HTTP and
WebSocket admission, upload deadlines, bounded OpenHAB reads and explicit redirect
rejection. It does not close F11 idle/replay behavior or F12 observation correctness.
The rolling daily budget and historical storage policy require operator sizing
review; recovery is available even when new issuance is refused. Final source-
pinned results belong in the VPS evidence before accepting the gate.
