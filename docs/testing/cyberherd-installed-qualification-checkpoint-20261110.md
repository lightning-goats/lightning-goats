# Installed-role OS qualifier — 2026-11-10

Qualified code `320eeafb926a3f1cac23d405a123873e35c3c19b` and tested merge
`788a5ddccd71c591596ce16ddcf291f087daffa7` share tree
`92757662088d4337ef455f2bdb654fb14b9208ff`.
The [fourteen-file authenticated qualifier](../deployment/cyberherd-installed-qualification.md)
has passed both disposable-systemd CI and its actual installed-host OS checks.
The original CI-only harness remains CI-only; its guard was not relaxed.

[CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42843862734),
job `129925948686`, passed format, strict locked all-target/all-feature Clippy and
the locked all-feature suite: **495 passed, zero failed, four ignored**. Four
required separate root markers passed. Root repeats and inactive example tests
remain excluded from 495.
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42843862454),
job `129925948977`, passed its locked dependency, original SQLite provenance and
RSA-unreachability gates without a SQLite waiver.
[Isolated real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42843862642)
also passed its independent synthetic fixtures.

[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42843862436)
passed all five jobs. General nginx discovery ran 181 tests with 36 environment
skips; six new qualifier tests passed, and 31 separate root tests passed.
The root launcher fixture exercised the new entry point and substitution guards.
Systemd job `129928299742` ran the actual authenticated qualifier after fresh
three-role initialization, before the separate older CI-only policy probes.

Systemd artifact `13777238187`, ZIP SHA256
`eedd09865744f9770311701e3bf025b7e173dd50b2152ea89cd85d6f4269ed30`,
was downloaded and its outer digest/internal checksums verified. Its nested
qualifier receipt positively accepted all seven required fields for each role,
both public mock families and private-address denial, unchanged journals/core
unit, no application workers and no real API/Nostr credentials. DNS, Rust socket
protocol, household and provider qualification remained explicitly false.

After that qualification, every exported tool was checked against the pinned
git object. The launcher and tool manifest were independently authenticated
before root interpretation. A separate protected tool generation was installed;
the prior generation and credential-import command were retained. The initial
unprivileged export used an incorrect full commit string and refused it before
creating output. The corrected literal git-object identity above exported and
verified successfully; no unverified generation was installed.

On the actual VPS, the exact authenticated command ran under a fresh network
namespace against the [accepted inactive installation](cyberherd-live-inactive-installation-20261110.md).
It verified original receipt/archive/program/configuration/journal bytes,
actual NSS identities, zero/disabled policy, persistent masks and absence of
other role processes. Existing role runtime directories and recovery/sidecar
state would have refused the fresh-install probe.

All three actual Python probes passed UID/GID/groups, zero kernel capabilities,
NoNewPrivs, exact synthetic credential scope, foreign-state/credential-store
denial, immutable managed files, and public IPv4/IPv6 versus private IPv4/IPv6
mock access. Root positive controls proved all four isolated mock endpoints
reachable before attributing denial to the role policy. All synthetic ciphertext
was created under the new private qualification directory; no production API or
Nostr credential was read or changed.

The accepted private receipt has `actual_installed_os_policy_probes=3`,
`journals_unchanged=true`, `core_process_and_unit_unchanged=true`,
`application_workers_started=false` and `real_credentials_loaded=false`.
After cleanup, all original role masks/configurations/programs/journals still
matched, no role process remained, and the receive unit/configuration/process
were unchanged. No application worker, test payment, grant, project note or
feeder request was started/sent. Root-private baseline/results remain retained.

This is **actual-host OS policy qualification with Python probes**. It does not
prove actual Rust workers/socket protocol, production DNS or host-global
firewall, household signing/kinds, provider transfers, browser behavior or
recovery adoption. Those work items and explicit nonzero-budget/publication/
financial activation remain unfinished. Keep the full twelve-part goal active
and PR111 draft.
