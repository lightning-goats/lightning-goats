# Cashu release candidate

Production remains on hold. Build the review candidate from a clean source checkout.
The archive contains the core daemon and CLI, feeder gateway, wallet daemon,
Cashu signer and wallet state tool. It includes the inactive wallet and signer
unit examples, shadow core configuration, nginx example, operator runbooks,
backup tools, the Cashu overlay and its assets, and the locked Rust dependency
notices. The bundled QR code library includes its MIT notice and source record.
Provider credentials, wallet state and private keys are provisioned separately
through the approved procedures.

```sh
cargo fetch --locked
cargo fetch --locked --manifest-path tools/cashu-compat/Cargo.toml
python3 deploy/scripts/package-cashu.py build /tmp/lg-cashu-review
(
  cd /tmp/lg-cashu-review
  sha256sum --check SHA256SUMS
)
python3 deploy/scripts/package-cashu.py verify /tmp/lg-cashu-review/*.tar.gz "$(git rev-parse HEAD)"
```

Use Rust and Cargo 1.88, a native x86-64 GNU/Linux host, GNU binutils, a C
compiler, Python 3.11 or later and the cached locked crates. The builder uses
`--release` by default through Cargo's release profile. Supply `--profile dev`
for a local build rehearsal. Both profiles compile offline after the cache is
populated. The deployment workflow assembles the release profile from the
selected Git checkout. Run the verifier against the independently selected
source commit and artifact checksum.

`BUILD-INFO.json` records the source tree, tool versions and executable hashes.
Each executable records Cargo packages whose archive members GNU ld selected,
Rust toolchain and system archive members, the ELF shared-library requirements
and the highest required numeric glibc symbol version. The archive selection
table may include objects whose sections the linker subsequently discarded.
Build-only and other locked packages retain notice coverage in the complete
offline export. The packaged Rust toolchain notices accompany the selected
standard library code. The host supplies system libraries. Use the existing
source-pinned procedures for external tools such as `nak` and `age`.

The `cashu-backup-capsule/` directory contains the source-pinned backup and
maintenance programs with their source-bound `TOOL.json`. The archive verifier
checks the capsule manifest against the selected source and its packaged tools.
Use the approved ownership and authentication procedure in the [backup runbook](cashu-backup-collection.md).
Coordinated recovery also needs the matching CyberHerd binaries from the same
workflow checkout and the verified external `age` executable.

The builder runs `--help` on each trusted build and writes the archive with
ordered members, normalized modes and Git-commit timestamps. The tar payload
reassembles deterministically with the same inputs. Gzip bytes can vary with the
compression implementation. Independent compiled-binary reproducibility across
hosts remains unverified. The verifier reads the
archive directly and checks its contents, hashes, lock inventories, source
identity and ELF headers. Installed startup and live provider or feeder
acceptance follow their existing verification procedures.

The separate `cashu-systemd` job uses the same release archive in a disposable
Ubuntu 24.04 runner. It exercises the packaged wallet and signer under their
service sandbox properties with isolated paths, two synthetic runtime identities,
and a host-encrypted synthetic signer credential. Both must start and stop twice
with IP denied even to a listening loopback socket. A fenced restart must open no
IPC listener and preserve the retained state. The job uses newly initialized empty
fixtures. Funded checkpoint installation and reconciliation remain separate.

## Add the inactive Cashu schema to an existing core

Use the authenticated root-owned VPS upgrade tool closure with the paired core
release and CyberHerd review archives from the same verified source commit.
Pass their independently verified SHA-256 digests. A Cashu upgrade preserves
the original configuration and the original active service set. The Cashu
wallet and broker must be absent or persistently masked. The initial core
configuration must have no Cashu policy.

The launcher accepts these Cashu operations:

```text
--cashu-core-prepare CORE_ARCHIVE REVIEW_ARCHIVE SOURCE CORE_SHA256 REVIEW_SHA256 REVIEW_PATH
--cashu-core-maintenance plan REVIEW_PATH EXECUTION_PATH --review-sha256 PREPARED_SHA256
--cashu-core-maintenance apply EXECUTION_PATH --exclusive-maintenance
--cashu-core-maintenance resume EXECUTION_PATH --approve-resumption
```

Preparation and planning preserve the running services. Application stops and
masks the four journal writers, takes a fresh native schema-25 snapshot, and
migrates a separate clone under the original runtime UID with networking
isolated. The native comparison requires unchanged original rows, row IDs,
schemas and signed bytes, plus the compiled empty Cashu schema. Application
then installs the selected programs, migrates the fenced original and repeats
the native comparison. An interrupted migration retains its evidence and stays
fenced. Resumption verifies the same comparison and restores only the original
active roles in service dependency order, keeping the Cashu units inactive.
