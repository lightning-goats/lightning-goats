# Inactive upgrade transaction candidate

Production HOLD. `deploy/scripts/inactive_file_transaction.py` is a library
primitive for a reviewed inactive-upgrade coordinator, not a host installer or
an activation command. There is deliberately no command-line entry point. The
existing fresh installer still refuses preexisting accounts and installation
paths. This candidate has been exercised only with disposable files.

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
or untrusted manifests. The host coordinator remains to be implemented and must:

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
permissions. Host application remains unimplemented and unaccepted.
