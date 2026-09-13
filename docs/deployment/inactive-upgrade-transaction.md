# Inactive upgrade transaction candidate

Production HOLD. `deploy/scripts/inactive_file_transaction.py` is a library
primitive for a reviewed inactive-upgrade coordinator, not a host installer or
an activation command. There is deliberately no command-line entry point. The
existing fresh installer still refuses preexisting accounts and installation
paths. This candidate has been exercised only with disposable files.

The companion `upgrade-vps-canary.py` now supplies the fixed-destination host
coordinator described below. Its snapshot has been tested read-only on the VPS;
prepare/apply/rollback have run only in a disposable fixture. No persistent
upgrade or activation acceptance is claimed.

The primitive accepts fixed destination/source/expected-fingerprint tuples. It
preserves old bytes, modes, ownership and extended attributes (including a
SELinux label when present) and stages the new bytes in a fresh private
transaction directory. It fsyncs backups and the immutable manifest before any
replacement. A nonblocking lock excludes other executors of that transaction.
Apply and rollback verify every backup, staged file and destination before the
first mutation. Each replacement uses a private temporary file on the destination
filesystem, verifies its full fingerprint, atomically renames and fsyncs the
parent. Existing state outside the fixed destination set is never traversed.

Recovery is based on actual old/new destination fingerprints, not an in-memory
progress counter. A process may die after a rename but before reporting it;
resuming recognizes the new fingerprint and does not destroy the retained old
copy. Rollback restores only files matching one of the recorded states. Unknown
bytes/metadata require manual reconciliation. A partial prepare without the
manifest/lock is not executable and must be preserved for inspection. Do not
remove transaction evidence merely because the service is still inactive.

## Required caller boundary before host use

The primitive is not safe as a generic privileged API accepting arbitrary paths
or untrusted manifests. The host coordinator enforces the following boundary;
administrative exclusivity remains an operator precondition:

- Use only the reviewed daemon, CLI and canary-example destinations. Reject
  aliases, symlink/untrusted parents, unexpected modes/owners/ACLs and any input
  not bound to independently verified archive/source evidence.
- Keep the transaction directory and all ancestors under exclusive administrative
  control; validate its immutable manifest, fixed destinations and metadata on
  every resumed operation. Exclude competing administrative file changes and
  service activation for the whole maintenance session. The transaction lock
  does not lock out an administrator or systemd.
- Implement `guard()` to verify inactive/disabled services, no runtime processes,
  the reviewed runtime account/permissions, unchanged effective unit/drop-in
  configuration and the approved empty-state/configuration baseline. The library
  invokes this before preparation, before and immediately preceding replacement,
  and after completion; it performs no service stop, mask, reload or start.
- Capture the historical installation record separately, verify effective runtime
  write denial and installed `--help` under the non-admin UID, and fsync a source-
  pinned completion receipt only after final checks. Preserve failed receipts and
  backups. Do not turn a file-operation return value into deployment acceptance.

The selected inactive upgrade changes no unit, database or credentials. Later
active sessions need their own quiesced paired-store backup/restore and approval.
For cross-host rollback, close HOME canary exposure before removing its peer
identity binding. Replacing local binaries is not that network rollback.

## Regression evidence

`python3 -m unittest discover -s deploy/tests -p test_inactive_file_transaction.py -v`
tests real disposable files and a subprocess killed with `os._exit` after its
first rename. It verifies resume, rollback, metadata/xattr preservation, unrelated
state preservation, backup corruption/drift rejection, concurrent-executor
rejection and guard failure between replacements. These tests do not establish
power-loss behavior on every filesystem or actual installed account/sandbox
permissions. Actual host application remains unperformed and unaccepted.

## Reviewed coordinator procedure

Use the exact reviewed coordinator revision and independently reviewed release
archive/source/digest. Keep all baseline/transaction inventory private. This
procedure is only for the original empty, inactive installation; it deliberately
rejects any active configuration, credentials, existing runtime process, nonempty
state, enabled service, production unit, manager-reload requirement or changed
unit/drop-in. Unexpected ACLs, capabilities, symlink parents and file modes are
also refused. It never stops services to make these checks pass.

First capture the read-only baseline and review the complete result against the
authorized inventory, including the effective drop-in set:

```sh
sudo python3 -B deploy/scripts/upgrade-vps-canary.py snapshot > /private/baseline.json
sha256sum /private/baseline.json
```

Successful exit and valid JSON are required. A digest only pins that reviewed
snapshot; it does not by itself approve the baseline. After candidate review and
an exclusive maintenance window, prepare the retained transaction:

```sh
sudo python3 -B deploy/scripts/upgrade-vps-canary.py prepare \
  /path/to/reviewed-release.tar.gz FULL_ARCHIVE_SOURCE_SHA ARCHIVE_SHA256 \
  /private/baseline.json REVIEWED_BASELINE_SHA256
```

This creates one fresh root-private directory under
`/var/lib/lightning-goats-upgrades/`, verifies the archive's private copy and
source, requires its shipped unit to match the installed unit, and retains
backups/staged replacements plus the baseline. It does not replace installed
files. Preserve any failed/partial directory for inspection; do not reuse it.

Use exactly the prepared UUID directory returned by that command:

```sh
sudo python3 -B deploy/scripts/upgrade-vps-canary.py apply /var/lib/lightning-goats-upgrades/UUID
# If a reviewed rollback is required while the original empty/inactive
# preconditions still hold:
sudo python3 -B deploy/scripts/upgrade-vps-canary.py rollback /var/lib/lightning-goats-upgrades/UUID
```

Both commands validate fixed destinations and old/new metadata against the
root-protected preparation record before executing the transaction. They recheck
the full host baseline around each replacement, run permission/write probes and
the installed binaries' `--help` as the non-admin runtime user, and retain a
durable result with actual installed fingerprints. A rollback receipt identifies
the prepared archive separately from the restored file fingerprints; it does
not falsely label restored bytes as the new source. The historical installation
record is preserved. A failed final probe leaves files/backups for reconciliation
and creates no success receipt; nothing automatically activates or rolls forward.

There is no service stop/start/enable/reload, unit replacement, credential load or
database conversion. This helper cannot be used to roll back a later nonempty
active session; use the separately reviewed paired-store reconciliation procedure.
Do not start another administrative transaction or activate a service during
these commands. Root administrators are outside the per-transaction lock.

Deployment CI runs the coordinator fixture separately as root in a fresh network
namespace: all target/state/transaction paths are redirected to a disposable
`/run` tree, service/account inventory is mocked, and real non-root permission/
help probes and file apply/rollback execute. The ordinary unprivileged test run
skips that fixture; the separate root step must pass with no skipped fixture
before claiming coordinator integration evidence.
