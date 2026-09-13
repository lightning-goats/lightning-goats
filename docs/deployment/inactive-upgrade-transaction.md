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

### Authenticate the tool before interpreting Python

Do not run this coordinator or its helpers as root from a checkout. The reviewed
bootstrap is `launch-vps-upgrade.py`; it and all five local/transitive modules
must be copied into a fresh root-owned directory under an exclusively root-owned,
non-writable ancestor chain. The exact bundle contains only:

- `launch-vps-upgrade.py`
- `upgrade-vps-canary.py`
- `inactive_file_transaction.py`
- `preflight-release.py`
- `prepare-vps-canary.py`
- `smoke-release.py`
- `TOOL.json`

Export the exact reviewed commit without running its files:

```sh
python3 -I -S -B deploy/scripts/export-upgrade-tools.py /path/to/repository FULL_REVIEWED_SHA /private/new-tool-bundle
```

The exporter refuses root execution and existing destinations. It reads regular
Git blobs from the literal commit, disables replacement objects and caller Git
environment overrides, and ignores dirty checkout files. It writes the six tools
and deterministic `TOOL.json` with private modes and prints the manifest/launcher
hashes. Failed output directories must be retained for inspection. This is an
unprivileged preparation utility, not a trusted installer or source approval.

Generate `TOOL.json` from the exact reviewed commit's file bytes, without running
those files. Its schema is `{"version":1,"source_commit":"FULL_SHA","files":
{"FILENAME":"SHA256",...}}`; `files` must cover exactly the six Python files,
including the launcher. Independently review the source commit, every file hash,
the manifest SHA256 and the launcher SHA256. A manifest supplied alongside files
is not independently authenticated evidence. No checkout code may run with sudo
to construct or establish this trust boundary.

Using trusted system installation tools and the approved maintenance procedure,
create a fresh root:root mode-0700 bundle directory under a root-controlled
installation prefix; copy the seven files as root:root mode-0600. Check every
ancestor for ownership, writability, symlinks and ACLs. Do not reuse or overwrite
a prior generation. Reject links, extra files and bytecode caches. Exclude other
administrative writers throughout installation and execution. Verify the copied
launcher with `/usr/bin/sha256sum` against the independently reviewed launcher
hash **before Python interprets it**. Verify the copied manifest hash as well.
Do not substitute a helper's self-check for this external bootstrap check.

Only after that boundary has been established, use the following command shape
with the actual root-owned bundle path and externally reviewed manifest digest:

```sh
sudo /usr/bin/python3 -I -S -B /ROOT_OWNED_BUNDLE/launch-vps-upgrade.py TOOL_MANIFEST_SHA256 snapshot > /private/baseline.json
```

The root-installed launcher rejects writable/aliased paths and unexpected bundle
contents, validates every module hash before loading any local code, and runs
with caller import paths and site initialization disabled. It rechecks the tool
identity during host operations. Root administrators remain outside this trust
boundary; this does not provide protection against a concurrent root writer.

Capture and independently review the baseline and its SHA256. Substitute the
same authenticated launcher prefix for the commands below:

```text
LAUNCHER prepare /path/to/reviewed-release.tar.gz FULL_ARCHIVE_SOURCE_SHA ARCHIVE_SHA256 /private/baseline.json REVIEWED_BASELINE_SHA256
LAUNCHER apply /var/lib/lightning-goats-upgrades/UUID
LAUNCHER rollback /var/lib/lightning-goats-upgrades/UUID
```

`LAUNCHER` is notation for the complete verified invocation above, not a shell
executable. Preparation retains backups and staged replacements without changing
installed files. Version-2 `UPGRADE.json` binds the source/tool manifest identity;
apply and rollback refuse a missing or different generation before replacement.
Results retain that identity alongside actual installed fingerprints.

Existing version-1 prepared transactions are retained unchanged as evidence and
are refused by this candidate. Do not fill in a tool identity retrospectively,
delete their backups, or automatically reprepare them. Their future disposition
needs a separately reviewed migration/reconciliation procedure.

All original inactivity, fixed-destination, archive, runtime permission and
rollback checks remain required. No command activates a service or changes a
unit, credential or database. The exclusive administrative maintenance window
and concrete harmless network/test approvals remain separate prerequisites.

Deployment CI runs `test_upgrade*.py` separately as root in a fresh network
namespace. The coordinator fixture redirects all installation paths into a
fresh `/run` tree and explicitly mocks inventory/tool identity, while real file
transactions and non-root probes execute. Separate actual launcher subprocesses
exercise authenticated `--help`, poisoned caller imports, substituted/writable/
symlinked helpers, unexpected bytecode directories and manifest drift. These
fixtures do not establish persistent installation or cross-host acceptance.
