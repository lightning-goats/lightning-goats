# Legacy core upgrade evidence — 2026-11-10

Verified code `eef127fa923d464c3f99c20284ba5a0b676d3103` and tested merge
`d1633332913edbed2e0e22bf99fe9486401031dd` have the same Git-data tree,
`f10aa57b919bd15c2fb73698aad5e8153db275e0`.
This qualifies the [native legacy backup/comparison commands](../deployment/cyberherd-core-upgrade.md),
not an installed upgrade, migration, restart or financial activation.

[CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42811136379),
job `129812556284`, passed format, strict locked all-target/all-feature Clippy
and the locked all-feature suite: **494 passed, zero failed, four ignored**.
Four root repeats and two inactive example tests are excluded from 494. The
separate mandatory root harness positively verified all four completion markers,
including `ROOT_LEGACY_CORE_UPGRADE_FIXTURE_VERIFIED`. The ordinary ignored
bootstrap/legacy fixtures run there; the existing real-nak fixture has its own job.

The native legacy fixture uses a synthetic historical schema through migration
13, a nonempty WAL, an original paid receipt/credit, pending physical UUID,
epoch and exact signed outbox bytes. Read-only raw main/WAL copying leaves all
original main/WAL/SHM bytes unchanged. Only a separate copy is migrated using
the actual current native store constructor. Comparison preserves every old
column/row/value and sequence, rejects changed embedded-NUL tails, wrong epochs,
incomplete copies, corruption and dirty/skipped/unknown/financial history.
Fixture quiescence observation is injected: it does not qualify a real systemd
stop/mask transaction, runtime account or production paid-store migration.

New CLI policy/backup/compare paths branch before application configuration and
the ordinary store constructor. The historical Python receive-upgrade helper
refuses CyberHerd financial archives before service commands; its system SQLite
inspection is not qualified by the native engine check. Neither path bypasses
the four-journal recovery boundary once financial schema exists.

[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42811136396)
passed all five jobs. Package job `129812556630` passed **18 non-skipped CLI
checks** and assembled an inactive **four-binary/103-file** review archive,
`activation=none`. Browser job `129812556750` passed **28 mocked-network tests**.
Nginx job `129812556599` discovered 151 tests with 23 environment skips; its
separate nine root upgrade fixtures passed. Existing core install/systemd tests
do not qualify installation of CyberHerd roles.

[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42811136300),
job `129812555906`, passed with 315 locked dependencies, 1,430 advisories,
original-driver provenance and actual RSA-unreachable checks. Native SQLite
remains 3.54.1, without a SQLite advisory waiver. This does not update installed
programs or Python/system SQLite.
[Isolated real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42811136344),
job `129812556438`, passed without qualifying new household clients/relays.

No actual service, configuration, credential, payment, credit, feeding or network
state changed. Keep PR111 draft and the twelve-part goal active. The reviewed
inactive installer, real paid-core transaction and installed identity/network/
provider/helper/household/browser qualification, external recovery adoption and
explicit financial activation remain required.
