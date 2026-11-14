# Patched SQLite source checkpoint — 2026-11-10

Feature code `6bc24d132577133121804f45fbd193ac39d9e249` was tested as PR merge
`eafd643f3d5ae3bb36bb096cdc1fb0a8e813fa88`. Independent local and GitHub Git-data
reads confirmed both trees equal `636536ffe83cc809e8c608b9b38812beef331942`.
The review archive records the actual tested merge checkout.

[CI 42789443629](https://github.com/lightning-goats/lightning-goats/actions/runs/42789443629),
job `129738809200`, passed format, strict locked all-target/all-feature Clippy and
the full locked all-feature suite: **486 passed, zero failed, one ignored**.
The separately repeated root-ownership test and two inactive example tests are
excluded from 486. The native
`sqlite::tests::native_engine_is_the_reviewed_sqlite_release_with_the_wal_memory_fix`
test passed after querying the actual SQLx connection for exactly SQLite 3.54.1.
The root fixture again emitted `ROOT_OWNERSHIP_FIXTURE_VERIFIED` and passed.

The complete suite retains settlement, migration, feed UUID, paid-credit, signed
outbox, lost-ACK, cost-review and legacy process-restore regressions. No application
schema or migration changed. The dependency update does not rewrite any real state.

[Security 42789443635](https://github.com/lightning-goats/lightning-goats/actions/runs/42789443635),
job `129738808302`, **passed**. The refreshed database contained 1,430 advisories;
315 locked dependencies were scanned. The actual RSA reverse-tree check printed
no active dependency, preserving the existing conditional RUSTSEC-2023-0071
exception. No SQLite waiver was added. The previously failing RUSTSEC-2026-0459
dependency gate is resolved in this source by `libsqlite3-sys 0.39.1`.

The offline provenance check passed before compilation and audit. It compared all
56 SQLx SQLite 0.8.6 files against the retained published archive with its official
registry checksum. Only the normalized manifest's native binding pin changes;
Rust sources, original manifest, VCS metadata and both licenses are unchanged.
The root crate remains on Rust 1.88 and SQLx 0.8.6, with its original unsafe-code
prohibition. Lockfile changes are limited to the native binding version/checksum
and the driver's local source resolution; the other SQLx packages stay at 0.8.6.

[Deployment 42789443896](https://github.com/lightning-goats/lightning-goats/actions/runs/42789443896)
passed all five jobs. Package job `129738814491` built all release binaries,
passed **eleven non-skipped CLI checks**, and verified a disabled three-binary,
**97-file** CyberHerd archive with `activation=none`. Both package builders checked
the staged driver provenance. The CyberHerd archive verifier independently
checked its embedded source/origin/lockfile payload. The added files are the
retained dependency source, original archive, licenses, README and verifier.

Browser job `129738814178` passed **28 mocked-network tests**. Nginx job
`129738814511` ran the four new provenance tests: changed code/license/manifest/
archive, missing/extra files, filesystem symlinks and changed application/lock
bindings fail. Its 143-test discovery includes preexisting environment skips;
it is not 143 non-skipped checks. The separate nine root upgrade fixtures passed.
Existing core systemd qualification passed in job `129740486694`; this does not
install or qualify the new CyberHerd identities on the actual host.

[Real NIP-46 42789443443](https://github.com/lightning-goats/lightning-goats/actions/runs/42789443443),
job `129738809093`, passed the existing isolated real-nak regression. The patched
storage dependency does not establish new household-client or relay permissions.

Local locked all-target/all-feature type checking also passed. The local
315-dependency audit and RSA reverse-tree command completed successfully, with no
new advisory exception. The current upstream CyberHerd explanation was fetched
again and its raw SHA256 still matches the pinned implementation plan.

See [the backport contract](../security/sqlite-binding-backport.md). This result
covers the reviewed source and built artifacts, not already installed binaries,
Python's system SQLite or unrelated programs. No live service/configuration/key,
financial review, payment, public publication or physical feeding was changed.

Keep PR111 draft and the full twelve-part goal active. Consistent whole-stack
backup/restore and retention operations, reviewed inactive installation, actual
identity/network/helper/provider/household-signer/browser qualification and
explicit financial activation remain required. Whole-stack recovery must use a
qualified parser and must not silently resume transfers from rolled-back journals.
