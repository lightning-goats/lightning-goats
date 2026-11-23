# CyberHerd outgoing runtime checkpoint — 2026-11-10

Feature code: `54487454e9c440c8ad706c3661eba96e857ab0d7`. GitHub tested PR merge
`67bb0ddc7a454cd61d54bfc0e099b7207da695c9` into base
`18e0261add12d78b6e567f471bb310ba8aec3898`. Independent comparison confirmed
both trees are `f648fea8e55691766479b18acca90cf2e2ce9c89`. The review archive
records the actual tested merge SHA. This is source/fixture verification, not a
live installation or completion of the twelve-part CyberHerd port.

## Completed checks

[CI 42745726821](https://github.com/lightning-goats/lightning-goats/actions/runs/42745726821),
job `129597229594`, passed formatting, strict locked all-target/all-feature Clippy
and `cargo test --locked --all-features`: **420 passed, zero failed, one ignored**.
The separate inactive preparation example passed **two tests** and its non-applying
artifact check; those two are excluded from the 420 suite count.

The completed log explicitly shows all ten new tests passing: broker unsigned
and signed restart recovery; signature/peer/purpose rejection; client/project
identity separation; rate/zero policy; lost execute response/restart; disabled
execution recovery; supported NIP-57 callbacks; reserved/no-payout export; and a
lost intake checkpoint. The latter is a test-only protected-journal fixture with
an unavailable prior core, not proof of automatic financial snapshot recovery.

[Deployment artifacts 42745726825](https://github.com/lightning-goats/lightning-goats/actions/runs/42745726825)
passed all jobs. Package job `129597229868` built the new binaries, ran **seven
non-skipped CyberHerd CLI checks**, assembled the separate three-binary review
archive and verified its checksums, source, disabled configuration and unit
identity declarations. Its 25 files record `activation=none`. Existing browser,
nginx, harmless owner, release/isolated-identity and systemd fixtures passed too.
Those systemd fixtures exercise the existing core package; they do **not** prove
installation of the three new CyberHerd roles.

[NIP-46 regression 42745727042](https://github.com/lightning-goats/lightning-goats/actions/runs/42745727042)
passed the existing isolated real-nak client/relay scenario. The new broker tests
use a synthetic backend with native signature checking; neither result qualifies
the new service against the live household bunker or production credentials.

[Security 42745727041](https://github.com/lightning-goats/lightning-goats/actions/runs/42745727041),
job `129597232770`, still fails on unignored **RUSTSEC-2026-0459** in
`libsqlite3-sys 0.30.1` (reported fixed at `>=0.39.0`). RSA remains unreachable.
This patch changes no dependency/lockfile and does not waive the finding.

## Financial recovery limit and remaining work

The [runtime](../architecture/cyberherd-payout-runtime.md) connects private plan
handoff, independent source qualification, journal-driven provider work and core
outcomes. It keeps the spend key out of core and the NIP-46 client key out of the
payout process. Neither service or financial operation was activated on production.

Normal process-crash recovery preserves execute intent and the original
quote/payment. **An older payout-only database rollback can lose that intent while
core retains only a reservation.** Do not resume execution from such a snapshot.
Before financial activation, add independent core-owned row/quote execution claims
and explicit restore/reconciliation tooling with rollback fixtures. Whole-stack
financial restores also require retained provider evidence and an explicit
epoch/recovery decision. Passing restart tests does not establish rollback safety.

Still unfinished: verified member-result delivery to intake, editable outgoing
zap wording and joining references, durable note/chat/overlay outboxes and real
publication receipts, lifecycle notices, roster/OBS and authenticated operator
interfaces, failed-cost recovery, installed cross-UID/network/helper qualification,
dependency remediation and explicit financial activation. The
[full contract](../planning/cyberherd-implementation-plan.md) remains the completion
standard. Keep the PR draft and execution disabled while the affected financial
recovery path is unfinished.
