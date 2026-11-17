# Cashu backup collection — copy normalization

The coordinated, encrypted production backup collector remains unfinished.
`lightning-goats-cashu-statectl normalize-copy` is its first native primitive.
The initial [local native checkpoint](../testing/cashu-copy-normalization-local-20261117.json)
passes two positive and 21 refusal/interruption cases. The later
[special-file correction](../testing/cashu-copy-special-files-20261117.json)
passes two positive and 24 refusal/interruption cases, including marker/main/WAL
FIFOs with three-second fixture deadlines; its selected-source CI is pending.
It opens only explicitly supplied private copies;
it neither stops services nor loads wallet keys. Do not use it on installed state.
The [recovery inventory](cashu-recovery-inventory.md) remains authoritative for
the complete checkpoint, historical policies and recovery fences.

Build from the locked, separate wallet dependency graph:

```sh
cargo build --locked --manifest-path tools/cashu-compat/Cargo.toml \
  --bin lightning-goats-cashu-statectl
```

The root collector must first prevent admission, verify all affected writers
have stopped, retain their writer authorities, and copy the complete database
and committed WAL sets into a fresh root-owned staging directory. These steps
are requirements for the future collector, not effects of this helper. Preserve
uncertain source sidecars and all original state. A main-file-only copy of a
database with committed WAL data is incomplete.

The stage must be a canonical absolute directory, mode 0700 or stricter, beneath
root-owned ancestors with no group/other write permission. Every input and marker
must be a root-owned, private regular file with one link; aliases and symlinks
refuse. Descriptor opens use `O_NONBLOCK` so FIFO inputs refuse promptly before
SQLite access. The stage must contain `INCOMPLETE`, mode 0600 or stricter, with exactly:

```text
LIGHTNING_GOATS_CASHU_BACKUP_COPY_V1
```

Retain the trailing newline. Raw basenames begin `raw-`; output basenames begin
`normalized-`. Both end in `.db` or `.sqlite` and contain only ASCII letters,
digits, dots, underscores or hyphens. The invocation is:

```text
lightning-goats-cashu-statectl normalize-copy PRIVATE_STAGE RAW_NAME NORMALIZED_NAME MAX_BYTES
```

`MAX_BYTES` is a numerical limit from 4096 through 17179869184 bytes. Main/WAL
inputs, logical database size and normalized output are bounded. A 256 MiB SQLite
heap limit and a 60-second SQLite/copy deadline also apply. The caller must impose
its own total process deadline and aggregate checkpoint/storage budget.

The pinned SQLite 3.54.1 backup API includes committed WAL transactions while
preserving implicit row IDs and signed byte values. The helper checks integrity
and foreign keys, requires a standalone output with no WAL, hashes source main
and WAL before/after, verifies output identity, and syncs the file and stage.
Input shared memory may be rebuilt only inside this copied stage. SQLite's Unix
VFS refreshes the copied WAL's ownership even when unchanged; its ctime may
change. WAL inode, ownership, permissions, link count, size, modification time
and complete bytes must still match. Main-file and marker ctime changes refuse.
Rollback
journals, orphan shared memory and any preexisting output/sidecar refuse.

Success emits a coarse JSON receipt with input/output hashes and size. Its
`quiescence_verified`, `coordinated_backup_verified`, `restore_authorized` and
`application_runtime_started` fields remain false. A checksum certifies copied
bytes, not agreement with current mint, Strike or physical outcomes.

Failure emits no private paths or database errors. Keep `INCOMPLETE`, raw copies
and any partial normalized output. Reusing an output refuses; do not delete it
to conceal a failed attempt. This primitive does not finalize a backup, erase
evidence, replace an installed database, clear recovery markers or restart work.

The root/network-isolated fixture
`deploy/scripts/test-cashu-copy-normalization.py` exercises main-only and committed
WAL copies, implicit row IDs and signed bytes, path/ownership/alias/size refusals,
foreign-key/corruption refusal and interrupted-output retention. CI runs the
actual selected native CLI. Synthetic fixtures do not establish production writer
quiescence, coordinated encrypted collection or funded restore.

The next collector must retain all native journals, the wallet journal and every
CDK namespace, both signer histories, historical keys/authentication, policies
and exact artifacts; verify their cross-store bindings; and encrypt the private
checkpoint. Wallet-only/core-only rollback and coordinated restore after external
effects still require separate qualification before any recovery fence can clear.
