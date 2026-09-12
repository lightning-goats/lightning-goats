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

Later source-pinned evidence, still awaiting integration and final acceptance:

- [PR59](https://github.com/lightning-goats/lightning-goats/pull/59),
  `b2f42dc39758d21d96068bcb0382db5314960908`: independent VPS review requests
  revisions for credential redirects, inspected-target binding and rehearsal
  state isolation. Green CI does not supersede this review disposition.
- [PR60](https://github.com/lightning-goats/lightning-goats/pull/60),
  `689f195a4376ddd9a845da80239c8cc3b8cfb0b0`: home-agent report of local harmless
  canary observations, including command counts and restore. This is attributed
  home evidence, not independently repeated VPS or cross-host acceptance.
- [PR61](https://github.com/lightning-goats/lightning-goats/pull/61),
  `1c09c8d6627960f1492ace31b0d3e94a99aae388`: a delayed unavailable safety read
  reproduces the restore test's inadequate five-second observation window;
  bounded observation and child-liveness checks preserve all accounting and
  command-count assertions. Rust, Security and Deployment passed. The original
  PR58 timeout cause remains unproven; its failed run remains historical evidence.
- [PR62](https://github.com/lightning-goats/lightning-goats/pull/62),
  `dbdd1a0136a4e2bf05a458e5adb439fb6ce5c50f`: six isolated WireGuard controls
  prove the specific peer binding rejects a spoofed staging source before host
  delivery, while legitimate traffic works. Removing that binding restores the
  spoof path. Rust, Security, Deployment and dedicated peer-authentication CI
  passed. No household rule, route or peer was changed.
- [PR63](https://github.com/lightning-goats/lightning-goats/pull/63),
  `3928378b89d55731f77166d45fe9bf3087843e58`: corrects a reproduced bunker
  credential-output disclosure and verifies real NIP-46 signing, signature
  rejection, durable outbox retry with the signer stopped, and overlay-only
  informational/weather behavior using synthetic keys and a local relay.
  Rust, Security, Deployment and dedicated real-NIP46 CI passed. This is not
  production signer identity, service or public-relay acceptance.

Check-run links are retained in the corresponding PRs and evidence documents.
These are separate candidates, not an assembled
release. Review their combined result and rerun the required gates before using
a final package; do not equate individual green PRs with integration acceptance.

| Required gate | Remaining work / responsible boundary |
| --- | --- |
| Owner finality | Home coordinator adopts/reviews PR57; verify actual atomic/timer/JDBC restore behavior, legacy callers and retention. Candidate stops at 32 entries and is not production-compatible yet. |
| Shared gateway protocol | Coordinate explicit v2 request/result and bounded history recovery before enabling it; shipped gateway remains v1. |
| Home canary contract | Contract received and protocol/team acknowledgement exchanged. Resolve PR59 review findings and verify the revised source before cross-host use. No token reaches the VPS. |
| Network authentication | Direct-peer design has isolated authentication evidence in PR62. Review the concrete home/VPS apply and rollback candidate, retire or separately contain inspection exceptions, then obtain approval and verify the live path. Source-IP filtering alone is insufficient. |
| Combined staging | Real daemon -> real home gateway -> harmless owner; synthetic 2340 sats -> two confirmed commands -> 340 sats, concurrency/failures/restart and paired restore. |
| Payment authority | Balance ceiling/manual-sweep responsibility supplied by the operator. Receive-only credential and webhook provisioning/scope evidence, actual balance check, independent recovery and final provider acceptance remain open. No real invoices/payments authorized. |
| Nostr | Isolated real signer/client acceptance passed in PR63; integrate its output correction. Production public identity/relay/credential contract and actual service sandbox acceptance remain open. Retain signed retry bytes. |
| Installed release | Agree final reviewed source/artifacts, verify binary provenance, prepare production units/config inactive, then authorized staging startup. Final production credentials and privilege revocation remain gated. |
| DNS/TLS and website | Preserve existing static TLS evidence and operator domain controls; reconcile final zone/CAA/SNI/renewal proposal. Page changes deferred; payment frontend acceptance remains open. |
| Cutover and rollback | Final matrix, quiesced paired backup, owner reconciliation, independent recovery, old-VPS archive, separately approved payments/physical tests and DNS/WireGuard change. |

The next joint step is the home agent's revised tooling and concrete authenticated
staging path candidate. The local harmless-canary contract has been received.
Do not open broad VPS access or substitute the physical owner for the canary.

## Operator decisions and remaining input

The operator confirmed the home agent is active and will perform manual sweeps
under the approved balance ceiling. Exact operational values remain in the
private operator checkpoint. Do not ask those questions again or imply the
daemon automatically enforces the balance policy. Project payment-credential
provisioning remains outstanding; do not request credential values in chat.

The production announcement public identity and relay URLs have been requested.
The website contact-link approval does not establish signer-migration authority.
Final credential installation, privilege reduction, network application and
live payment/physical acceptance need their concrete reviewed operator steps.

Missing inputs do not prevent independent repository/mock work. Never infer
approval from elapsed time or green CI. Keep parent issues #6/#15/#16 open.
