# CyberHerd inactive installation checkpoint — 2026-11-10

Verified code `6e7fd2eeb1e35c3d3176f09414397c11705758cd` and tested merge
`e4ea647d567a0fc2523f530cd10a31fd5de4f18d` share Git-data tree
`889fdc4be0d76aa020b9e195d2ef12d4653311e9`.
This advances the full twelve-part port; it does not complete or activate it.
The actual live host received no new service/account/configuration/credential,
payment/grant, public post, feeding, DNS or WireGuard change.

[CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42821547085),
job `129848044167`, passed format, strict locked all-target/all-feature Clippy and
the locked all-feature suite: **495 passed, zero failed, four ignored**. The
separate mandatory root harness positively verified all four required markers.
Four root repeats and two inactive example tests are excluded from 495. The
ignored native fixtures run separately; the existing real-nak fixture has its
own isolated job. A local build was interrupted during temporary-filesystem
pressure; its incomplete log is not reported as a passing exact-source suite.

The added regression verifies that committed copied WAL pages remain visible,
main/WAL bytes stay exact, and a checkpointed main-only image acquires no new
sidecar. The required native four-journal root fixture now also checkpoints only
additional scratch copies, verifies all four main-only layouts against the same
financial/replay report, and detects any new sidecar/byte change. Original
sources and accepted backup images are not checkpointed.

This fixes an actual installed-rehearsal failure: ordinary read-only inspection
could create an unlisted empty WAL beside a checkpointed image. The unlisted-file
guard remains strict. Only complete main-only copies use immutable mode; real
copied WAL files retain the regular read-only WAL reader and its permitted private
SHM index. Making a real WAL copy immutable would discard committed WAL evidence.

[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42821547187)
passed all five jobs. Package job `129848045179` passed **18 non-skipped CLI
checks** and produced the **four-binary/104-file** review archive with
`activation=none`. The test-only historical fixture generator is a separate
artifact and is absent from both production archives. Browser job `129848045323`
retains **28 mocked-network tests**. Nginx job `129848044817` discovered 173 tests
with 36 environment skips; its separate root upgrade/credential/client harness
passed **31 tests with no skips or failures**. These mock-manager filesystem
tests do not prove actual systemd sandbox behavior.

Systemd job `129849989746` also passed the **actual fresh inactive installer** on
its disposable Ubuntu 24.04 VM. The independently exported authenticated tool
closure ran real systemd-sysusers, NSS/group/password/sudo checks, persistent unit
masks/readback and three native initializers under distinct real non-root UIDs.
A new synthetic historical core in `/run` supplied native backup evidence through
an actually masked dummy unit. It never opened the original canary database.
The resulting private receipt positively reports:

- Three actual NSS identities, three masked units and three native initializers.
- Native SQLite 3.54.1 and zero CyberHerd units started/runtime credentials loaded.
- A root-only synthetic payout token encrypted/decrypted by the actual importer;
  its exact fingerprint binds the synthetic disabled policy. It is not a valid
  provider key and is never delivered to a runtime process.
- Provider, household and actual live-host qualification all remain **false**.

The retained systemd artifact is `13766917561`, ZIP SHA256
`b05b087507a754e1458e71b6db0134249cb6505949a3689ef16b66a599e1373e`.
Its synthetic JSON receipt was downloaded through the connector, the ZIP digest
checked and the receipt inspected. The core/review release artifacts were also
downloaded and their GitHub ZIP digests and inner manifests verified as local
data; no program was installed or executed from them on the live host.

Earlier rehearsal failures are retained in CI: Python 3.10 lacked stdlib TOML,
an inherited sysfs mount described the wrong network namespace, the tool parent
had a non-root group, and the main-only reader generated an empty WAL. Corrections
use Python 3.11+, actual netlink namespace inspection, root:root tool ancestors and
the conditional immutable reader. None weakens the corresponding guard. A native
example's private-module compilation error was also corrected without exporting
that application module. The synthetic client fixture restores its process umask
so later filesystem cases keep their own test setup.

[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42821547297),
job `129848045203`, passed with 315 locked dependencies and 1,430 advisories,
original-driver provenance and actual RSA-unreachable checks. No new dependency,
schema or advisory waiver was added.
[Isolated real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42821547174),
job `129848043786`, passed without qualifying the new household clients.

Prepared runbooks now cover the [explicit core upgrade](../deployment/cyberherd-core-upgrade.md),
[inactive installer](../deployment/cyberherd-inactive-installation.md),
[separate payout credential](../deployment/cyberherd-payout-credential.md) and
[distinct NIP-46 authentication clients](../deployment/cyberherd-nostr-clients.md).
The source-pinned thirteen-file tool closure was exported unprivileged as data,
without root installation or execution. Imported credentials/allowlists and
nonzero policies still require explicit operator steps.

Keep PR111 draft and the full goal active. The actual custom-core upgrade
transaction still needs a combined real-systemd/UID-migration rehearsal; its
filesystem coordinator and native comparison currently have separate evidence.
Actual live-host installation, effective socket/network/helper/sandbox controls,
provider/household/browser qualification, independently reconciled recovery
adoption and explicit financial activation remain required. No passing fixture
authorizes restoring an older paid ledger, retrying uncertain food/payments or
clearing a recovery marker.
