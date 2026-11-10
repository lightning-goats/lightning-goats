# Native evidence before upgrading the older core

The running phase-one core may predate CyberHerd migration 16. The four-journal
maintenance tool intentionally requires current schemas and cannot verify that
older database. `lightning-goatsctl` therefore provides separate native commands
for a **pre-CyberHerd core only**. They neither migrate nor restore state, start
services, load app configuration/credentials or authorize financial activation.
The [494-test checkpoint](../testing/cyberherd-core-upgrade-checkpoint-20261110.md)
records source verification; these commands are not installed here.

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
schema disabled. A complete checkpointed main-only copy uses immutable mode so
inspection cannot invent an empty WAL. If a copied WAL exists, the normal
read-only WAL reader preserves its committed pages and may create only the
already permitted private disposable SHM index; immutable mode must not ignore
that WAL. Both layouts retain exact image-byte checks. Integrity and foreign keys
must pass. Migration history must be
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

## Prepared transaction for the existing custom core unit

`upgrade-cyberherd-core.py` now coordinates the native commands through the
[authenticated root-owned tool launcher](inactive-upgrade-transaction.md).
The export closure includes this script; independently authenticate its pinned
commit, launcher digest, `TOOL.json` digest and release archive digest before
installing/executing it. Do not run a writable checkout or import archive Python
as root. This source has no permission to choose a maintenance window itself.

This bounded transaction supports only the existing receive configuration,
database and custom `/etc/systemd/system/lightning-goats-receive.service` unit.
Its baseline checks the effective core identity, non-admin groups, protected
program/config/unit/drop-ins and the original running service. New CyberHerd
units must still be absent: perform this legacy-core upgrade before their
inactive installation. It preserves the entire configuration and uploaded
artwork; only `lightning-goatsd` and `lightning-goatsctl` are replaced.

Create the protected root-owned mode-0700 upgrade base first if absent. Select
a **new** transaction path and independently verified source/archive pins:

```sh
sudo /usr/bin/python3 -I -S -B /ROOT_OWNED_BUNDLE/launch-vps-upgrade.py \
  TOOL_MANIFEST_SHA256 --cyberherd-core prepare \
  /PRIVATE/release.tar.gz FULL_SOURCE_COMMIT ARCHIVE_SHA256 \
  /var/lib/lightning-goats-upgrades/NEW_TRANSACTION
```

Prepare snapshots reviewed program/configuration/unit metadata and retained
original bytes, validates a private copy of the pinned archive and stages a
recoverable file transaction. It does not open the database or stop any service.
The original unit is retained before any mask operation. Failed preparation is
retained and never reused. Keep all transaction files private.

Only during the selected maintenance window, after excluding every other
administrative/helper/database writer, invoke:

```sh
sudo /usr/bin/python3 -I -S -B /ROOT_OWNED_BUNDLE/launch-vps-upgrade.py \
  TOOL_MANIFEST_SHA256 --cyberherd-core apply \
  /var/lib/lightning-goats-upgrades/NEW_TRANSACTION --writers-stopped
```

The attestation excludes other writers; the transaction itself stops the
original core. It atomically replaces the backed-up custom unit with a persistent
`/dev/null` mask, reloads systemd and positively checks mask/inactivity/PID/cgroup
and absence of any process under the original runtime UID. A runtime mask alone
would not override this custom unit. Protected configuration/drop-ins remain
unchanged throughout. No new CyberHerd unit/configuration/key is installed.

The native legacy main/WAL backup and verification must succeed **before** any
program replacement or migration. Then the file transaction installs the two
pinned programs with original metadata. The new `status` command is deliberately
used as the migration step, under the actual original UID/GID with empty groups,
no capabilities and NoNewPrivs. It loads no credentials and starts no workers.
Root takes a bounded raw post-migration main/WAL copy, observes SHM without
parsing the original, and asks the native verifier to compare it with the accepted
old bundle. Rollback journals, alias/ownership drift and changed source bytes fail.
New copies keep their destination filesystem label; raw copying never removes a
security label or re-labels an original database.

Only positive native acceptance writes `COMPARISON.json`. The original core
stays stopped and persistently masked on success. Failure/interruption keeps
the exact transaction and its exclusive phase markers for manual diagnosis;
`apply` cannot silently repeat. It never restores an old database, rolls back a
completed schema migration, rotates an epoch, retries food or erases signed work.
If interruption happened before masking, the core may be stopped with its
original unit still present: inspect actual unit state rather than assuming a
mask was established. Do not delete a marker to force another invocation.

After reviewing accepted evidence, original paid-state fingerprints and installed
programs, explicitly restore the exact original unit:

```sh
sudo /usr/bin/python3 -I -S -B /ROOT_OWNED_BUNDLE/launch-vps-upgrade.py \
  TOOL_MANIFEST_SHA256 --cyberherd-core finish \
  /var/lib/lightning-goats-upgrades/NEW_TRANSACTION --restore-original-unit
```

Finish rejects any post-comparison paid-state/program/configuration drift,
re-verifies native evidence and restores original unit bytes/metadata/drop-ins.
It reloads systemd and checks the core is still inactive/dead with PID zero.
It **does not start the core**. The operator retains a separate explicit restart
step and must assess accumulated feed credit/physical ambiguity before it.
No operation enables CyberHerd execution, public messaging or outgoing payments.

The disposable filesystem tests exercise actual file replacement, retained
configuration, custom-unit mask/restore, migration refusal, stale evidence and
no implicit restart, with systemd/native calls simulated. The Rust legacy fixture
separately exercises actual native migration/comparison. Neither fixture proves
this combined transaction on a real systemd host or production paid data.
Installed UID migration, effective unit handling, independent recovery adoption
and explicit restart still require qualification before actual installation.
