# Recovery primitives — qualification checkpoint, 2026-11-14

These changes preserve accepted backup directories and inventory original known
financial/physical/publication identities. They do not implement recovery adoption,
prove external history coverage, reconstruct missing financial evidence or clear
startup markers. No production operator binary, configuration, credential, paid
journal or service was replaced/restarted for the host qualification below.

## Restore output isolation

[PR #128](https://github.com/lightning-goats/lightning-goats/pull/128), source
`d352209d5489273deceea67d84361672d0f238b2`, is merged as
`37a118b358951eb83b6f0301f530fbbb58291fa9`. All required checks passed, including
[Rust 43639821722](https://github.com/lightning-goats/lightning-goats/actions/runs/43639821722)
and [deployment 43639821394](https://github.com/lightning-goats/lightning-goats/actions/runs/43639821394),
native systemd job `132586847419`.

Tested merge `4e5d82eb83e0b3a048fb87c166e42eff1eb40d43` supplied review artifact
`14130849524`. Comparing that tested merge with actual main finds only the two
historical-zap evidence documents changed by intervening PR #130; runtime/source
files are identical. The full trees differ, so no full-tree equality is claimed.
Its archive and all internal payload checksums passed. Retained
tar SHA256: `46dbeda2b86678a8eef59c07fb09c6961b11e4b5d3c2c6312c3355a541aac3da`;
operator SHA256: `91251360cdec2f3e2363e13477ddeb879fea776783bbbb8011a636eb8af0ac3f`.
A separate root-private copy refused the actual retained backup itself, a new
nested destination, and the backup parent. Each refusal occurred at the overlap
guard before writer/control checks or restore output creation. The nested path
was absent afterward; the accepted backup verified before/after, installed
program guards passed and four-service status was unchanged.

Protected host result:
`/var/lib/lightning-goats-upgrades/restore-overlap-d352209-review/RESULT.json`,
SHA256 `92e047e79867266c1cd6fb16cf14446cffbd5130c5d198f2339b27d97277bbea`.
The installed operator was not replaced; use the qualified separate candidate
for this corrected path until a reviewed operator update is selected.

## All-four-journal inventory

[PR #129](https://github.com/lightning-goats/lightning-goats/pull/129) extends the
[previous qualified inventory](cyberherd-recovery-inventory-checkpoint-20261114.md)
from 17 to 24 selections. Seven additions identify core signed notices/incoming
zap receipts, member unsigned/signed messages, lifecycle destinations including
cancellation, and payout signer intents/results. Original intent, message,
destination and event IDs stay distinct; no bodies/signatures/preimages are
exported and no replacement signature or public post is created.

Combined source `20b2e5f8187a410dc447164e0036b4b99de468e3` includes the merged
restore fix. [Rust 43640923037](https://github.com/lightning-goats/lightning-goats/actions/runs/43640923037)
passed strict locked Clippy, 438 library tests (six ignored), integrations and
actual-root checks. The new nonempty publication mapping/content-omission test
and extended root copied-WAL fixture passed. Audit/signer/home/network/website
checks passed. [Deployment 43640923103](https://github.com/lightning-goats/lightning-goats/actions/runs/43640923103)
also passed all jobs, including actual native systemd job `132589986764`.
All 24 queries also prepared against the actual four migrated schemas locally.

Review artifact `14132082742`, tested merge
`f72c99287cd801e0b83927963ff98763a0b403bf`, passed expected-source, archive and all
internal payload checksums. Retained tar SHA256:
`abd0b99bee4c4c7406ff595c89b8beb04af306809060e5ed83886e42813d3a27`;
operator SHA256: `9206ac5d79da52ddc32c763fb374b48279f44bb4bdcd0e1d2940f228032b08b3`.

Actual host qualification used this separate root-private candidate against the
existing accepted backup. All 24 inventory selections were present, output hashes
matched, the backup verified before/after, protected installed files and service
status were unchanged. External-history/adoption/resume authority remained false.
Protected host result:
`/var/lib/lightning-goats-upgrades/recovery-publication-20b2e5f-review/RESULT.json`,
SHA256 `ea730c34e6e376394a5604017667027daebccb2c1a892a1f4d8124976fc47918`.
This is backup-only inspection, not installed recovery adoption or external payment
acceptance. PR #129 is merged as `043334454ffeb62b1cf07966cabdd34e24ad2f4d`.
The tested merge and actual main have identical tree
`d5ed870e6c3f7f8743abad4fe33a890d0e4b08c0`.

## Remaining full-port work

Independent provider and household UUID history coverage/retention are not yet
qualified; the operator file-path request remains unanswered. The backup inventory
cannot discover effects missing from that backup. Concrete reconciliation,
missing immutable-evidence reconstruction, resolved-state installation and explicit
operator adoption remain unimplemented. Restore markers remain enforced.

Current-release zap/public-copy and real provider/payout/recipient acceptance,
private contact settlement observation and the remaining full-contract scopes
remain open in the [acceptance inventory](cyberherd-acceptance-matrix.md).
No real payment, physical feeding or membership reset was submitted for these tests.
