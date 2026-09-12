# VPS cutover status — 2026-09-12

**Not ready for cutover. Production HOLD.** This is the VPS-side counterpart to
[the home assignment](home-gateway-agent-handoff.md), not authorization to enable
payments, feeding, home access changes, DNS or hub cutover. Website work remains
deferred at the operator's request.

## Source and ownership

Reviewed repository main: `a74890c47a57a13508b3d259929f1f1bec2a5cc3`, including
integration merge `bf8749645c6e6942643a3b821cbb8804e7e200a8` and the parallel
home/VPS assignments. Main now contains PR31–55; the older instruction that it
lacks the stack is historical. Do not replay those patches.

[Draft PR57](https://github.com/lightning-goats/lightning-goats/pull/57), head
`5cd7976f78e747eaf7bc3cc130199353a891191b`, supplies the source-derived owner v2
candidate and containment policy/tests for home-agent adoption/review. It is not
installed and does not add a v2 gateway adapter. The home agent coordinates
physical-owner finality and home deployment; VPS source/API changes need the
shared pinned contract before either side changes protocols.

## Operational evidence handling

Detailed host inventory is retained in operator-controlled private evidence.
This public document records repository evidence and acceptance requirements;
it does not publish host topology, account metadata or deployment inventory.

## Proven software evidence versus open gates

PR57 current-head [Rust](https://github.com/lightning-goats/lightning-goats/actions/runs/34721487239),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/34721487245)
and [Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/34721487244)
passed. Evidence includes ten owner model regressions, nine isolated packet
cases, real TCP handshake/established-flow/rollback tests in disposable
namespaces, release packaging and systemd rehearsal. These do not establish
actual home OpenHAB/JDBC or physical behavior.

| Required gate | Remaining work / responsible boundary |
| --- | --- |
| Owner finality | Home coordinator adopts/reviews PR57; verify actual atomic/timer/JDBC restore behavior, legacy callers and retention. Candidate stops at 32 entries and is not production-compatible yet. |
| Shared gateway protocol | Coordinate explicit v2 request/result and bounded history recovery before enabling it; shipped gateway remains v1. |
| Home canary contract | Home agent returns observed source, harmless Items, URL, credentials/role, caps, weather and service status per its handoff. No token reaches the VPS. |
| Network authentication | Gateway currently has no application peer authentication. Coordinate end-to-end protection, staging versus final policy and old-hub impact. Source-IP filtering alone is insufficient. |
| Combined staging | Real daemon -> real home gateway -> harmless owner; synthetic 2340 sats -> two confirmed commands -> 340 sats, concurrency/failures/restart and paired restore. |
| Payment authority | Receive-only credential and webhook provisioning/scope evidence, independent recovery, approved balance ceiling/sweep procedure and final provider acceptance. No real invoices/payments authorized. |
| Nostr | Production signer identity/relay/credential contract and actual isolated signer acceptance; retain signed retry bytes. |
| Installed release | Agree final reviewed source/artifacts, verify binary provenance, prepare production units/config inactive, then authorized staging startup. Final production credentials and privilege revocation remain gated. |
| DNS/TLS and website | Preserve existing static TLS evidence and operator domain controls; reconcile final zone/CAA/SNI/renewal proposal. Page changes deferred; payment frontend acceptance remains open. |
| Cutover and rollback | Final matrix, quiesced paired backup, owner reconciliation, independent recovery, old-VPS archive, separately approved payments/physical tests and DNS/WireGuard change. |

The next joint step is the home agent's **local harmless-canary status contract**,
followed by an agreed authenticated cross-host connection. Until then, do not
open broad VPS access or substitute the physical owner for a missing canary.

## Pending operator input

Questions already presented, not answered in this checkpoint:

- Is the home agent actively working under the new assignment?
- What operational Strike balance ceiling and sweep procedure are approved?
- Are receive-only production/sandbox credentials provisioned (status or approved
  secret-file locations only; no secret values in chat or repository)?

None of these pending answers prevents independent repository/mock work. None
may be assumed from elapsed time. Keep parent issues #6/#15/#16 open.
