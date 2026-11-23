# CyberHerd stream chat opt-out — 2026-11-14

The operator requested that CyberHerd messages stop appearing in stream chat.
The independent `[messaging] stream_chat = false` policy preserves note-thread
publishing, standalone feeding/lifecycle notes and WebSocket/OBS presentation.
Existing configurations retain their prior behavior when the setting is omitted;
the example recommends explicitly disabling chat.

The policy is bound to the runtime store before it is cloned into workers. New
member, feeding and lifecycle messages omit stream destinations, and stream-only
invitations are not generated. Reservation queries exclude previously queued
stream destinations before the candidate limit, so retained chat cannot starve
note delivery. Workers also refuse kind-1311 publication with chat disabled.
Existing signed events, frozen text, leases, receipts and financial history are
retained; no chat is falsely marked delivered or erased. Re-enabling the policy
would deliberately permit retained chat retries again.

Regression coverage checks retained signed chat is never selected/sent, note
updates still publish, member overlays remain available, lifecycle notes remain
while invitations stop, and confirmed feeding notes/overlays survive with an
actual stream present. No financial schema or accounting change is required.

## Qualified and applied

[PR #117](https://github.com/lightning-goats/lightning-goats/pull/117) is merged
and deployed. Source `e8c8282258dfe2cc07fd289593480cb0f5e8d5ec` passed all ten
workflows, including strict Clippy, locked full-feature Rust tests (425 library
tests passed, six ignored, plus integrations/root checks), security audit,
real NIP-46 and installed systemd rehearsal. Rust run: `43617263992`; deployment:
`43617264055`, systemd job `132516607971`. The trusted review artifact was built
from tested merge `4e8de0d5c1008059aa3a3db55d1afabfdd9d94ec`.
ZIP SHA256: `b9c7027fac88b1c04c9df83665b521f75161d4c9838b1e9bcc11df247fc33532`;
verified review archive SHA256:
`5a1f83e7dabbb03b48be770b900c707d1006effe4edba9bcae9ed98dd0a51fb8`.

The native candidate accepted the changed configuration under the original
membership UID with isolated networking, no credentials and no journal access.
The protected two-file transaction replaced only `/usr/local/bin/cyberherdd`
and added `stream_chat = false` within `[messaging]` in
`/etc/cyberherd/config.toml`. Parsed configuration comparison required every other
field to remain equal. Installed worker SHA256:
`5e008909556b827eacc6f42e1defa7ad1b77cb4d0204af24d1dc640ce7dd32f0`.
Original target ownership, modes and SELinux labels were preserved.

The authorized messaging maintenance stopped/masked all four roles, took and
verified a fresh native four-journal backup, guarded actual journal fingerprints
unchanged through replacement, and reverified them before resumption. No schema,
financial policy, credential, feeder control, other program or unit changed.
Original service masks/enablement were restored and all four services resumed
under their original non-admin identities with successful health checks.

Protected evidence directory:
`/var/lib/lightning-goats-upgrades/cyberherd-no-stream-chat-e8c8282-review`.

- PREPARED: `931469b4b79f0bb966bf63599026d12ec89c28b86b29705ed710d7533844f577`
- APPLIED: `9ac5a349daf0c28134ecdedef4908ae102af836772a84983bbb30bbd04f4a9af`
- RESUMED: `74a1ce274e83968e88a5a27ed2d8f7d8a4fe56566ef0f9d6aa6b9b55bb497e40`
- PUBLICATION-POLICY-VERIFIED: `8aba03586c0705ff2dfee69863e2e5ae9a942c03521082595d6abb79a6999cbf`

## Live verification

Readback confirmed messaging and lifecycle notes enabled, with stream chat
explicitly disabled. A read-only membership-journal observation 89 seconds after
resumption found zero new member-chat or lifecycle-chat relay receipts despite
15 retained chat destinations. All four services were active/running with zero
restarts and zero main-process exit status. Existing chat history was retained;
no receipt was injected or chat marked delivered to hide the backlog.

There were no new note receipts during that observation window. Continued note
and overlay behavior is covered by the exact-source regressions and enabled
configuration; this checkpoint does not invent a new production note/feeding
transaction to prove it. Earlier successful chat publication in
[the public rollout record](cyberherd-discovery-repair-live-20261114.md) is historical;
future CyberHerd chat publication is now disabled.
