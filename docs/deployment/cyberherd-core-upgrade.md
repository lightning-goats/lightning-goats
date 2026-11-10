# Native evidence before upgrading the older core

The running phase-one core may predate CyberHerd migration 16. The four-journal
maintenance tool intentionally requires current schemas and cannot verify that
older database. `lightning-goatsctl` therefore provides separate native commands
for a **pre-CyberHerd core only**. They neither migrate nor restore state, start
services, load app configuration/credentials or authorize financial activation.
Source verification is in progress; these commands are not installed here.

Do not use the new ordinary `status` command to discover the old database before
backup: its normal store constructor applies current migrations. These explicit
commands branch before app configuration and that constructor. The existing
`upgrade-strike-receive.py` still uses system Python SQLite to inspect the original
database; its old apply path now refuses a CyberHerd financial archive before any
service command. It has not been qualified for the CyberHerd upgrade.
Do not mistake the patched Rust engine for qualification of that Python parser.

Resolve `deploy/cyberherd/core-upgrade.toml.example` using the actual existing
core database path, runtime UID/GID and system unit. Its zero IDs deliberately
fail validation. This policy needs no project/provider/key fields. Check it
without root, state or service access:

```sh
lightning-goatsctl check-core-upgrade --policy /path/to/reviewed-core-upgrade.toml
```

Install the reviewed binary and protected root-owned policy before operations.
During an operator-selected maintenance window, exclude every daemon/helper/
administrative writer, stop the core and verify its exact unit is inactive/dead,
PID zero, masked and its cgroup empty. A mask command returning success alone
is insufficient; custom unit placement can prevent a runtime mask taking effect.
Preserve the actual unit/drop-ins and use the reviewed host procedure rather than
overwriting a custom unit to force masking. The native command does not stop,
mask, unmask or restart anything; the no-other-writers attestation is required.

```sh
sudo /usr/local/bin/lightning-goatsctl backup-legacy-core \
  --policy /etc/lightning-goats/core-upgrade.toml \
  --destination /var/lib/lightning-goats-upgrades/NEW_PRIVATE_BUNDLE \
  --writers-stopped
```

The destination must be new under a protected root-owned parent. The source is
read through private, owner-checked, single-link read-only file descriptors.
Main and committed WAL bytes are copied and hashed without opening SQLite on
the original or creating root-owned original SHM. Source fingerprints, sidecar
presence and stopped/masked state are rechecked. Partial output retains
`INCOMPLETE`. An accepted root-private `MANIFEST.json` records scope, image hashes,
existing epoch, migration versions and logical table fingerprints. Stdout contains
only the operation/ID/acceptance and `resume_authorized=false`.

Only copies are parsed, using native SQLite 3.54.1 read-only/query-only with trusted
schema disabled. Integrity and foreign keys must pass. Migration history must be
an exact successful known prefix ending between versions 9 and 15; gaps, changed
checksums, dirty/future history and CyberHerd financial tables fail. A current or
financial core requires four-journal maintenance, even when its reward tables
look empty. This mode cannot bypass the later financial recovery boundary.

Logical fingerprints commit the original column names, row IDs, storage classes
and length-delimited values, including exact signed JSON, embedded NUL text,
blobs, full-width integers and real values. They also retain the original SQLite
sequence table. The private manifest contains no raw row dump or secret files.
Checksums detect changes but do not authenticate an untrusted backup; keep its
custody, installed binary/configuration/credential recovery evidence separately.

```sh
sudo /usr/local/bin/lightning-goatsctl verify-core-upgrade \
  --policy /etc/lightning-goats/core-upgrade.toml \
  --bundle /var/lib/lightning-goats-upgrades/NEW_PRIVATE_BUNDLE
```

Actual schema migration remains a separate reviewed step under the core runtime
UID while all writers remain stopped. Before any worker restart, obtain a separate
consistent post-migration copy in a new root-private directory named `core.db`
plus its optional `core.db-wal`, files mode 0600, without an `INCOMPLETE` marker.
Never copy a running database or parse original sources as root merely to compare.

```sh
sudo /usr/local/bin/lightning-goatsctl verify-core-upgrade \
  --policy /etc/lightning-goats/core-upgrade.toml \
  --bundle /var/lib/lightning-goats-upgrades/NEW_PRIVATE_BUNDLE \
  --candidate-copy /var/lib/lightning-goats-upgrades/NEW_POST_MIGRATION_COPY
```

Comparison revalidates the original bundle, candidate file identities/hashes,
current exact migration checksums, existing epoch and every original row/column
fingerprint. Added schema columns do not invent permission to rewrite old values.
New financial work must remain empty: this is a pre-activation comparison, not a
way to compare or restore an already active four-journal system. A failed compare
leaves both originals and copies untouched and services stopped under the
operator's maintenance procedure. No command applies migrations, installs copied
data, changes an epoch, releases a hold or restores an older paid ledger.

The remaining installer/upgrade transaction must qualify protected tool/archive
provenance, all writers and unit handling, actual UID migration, preserved
configuration/artwork, failure recovery and explicit restart. Passing this evidence
tool is not installed upgrade acceptance or permission to skip those steps.
