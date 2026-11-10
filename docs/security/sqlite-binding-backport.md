# Patched SQLite with the reviewed SQLx 0.8.6 driver

The isolated CyberHerd source updates `libsqlite3-sys` from 0.30.1 to exactly
0.39.1, which bundles SQLite 3.54.1. This addresses the native dependency reported
by [RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459.html):
older bundled engines can expose uninitialized memory when reading a database
with a crafted sibling WAL of a different page size. A provider or payment-policy
change is not needed to replace this native engine.

## Compatibility and exact provenance

The official registry currently offers SQLx SQLite 0.8.6 with a `^0.30.1` native
binding constraint. Its 0.9.0 release requires Rust 1.94 and also predates the fixed
binding. The patched 0.39.1 binding supports the application's Rust 1.88. Therefore
the backport retains SQLx 0.8.6 and changes just its SQLite driver's binding pin.
The application pins SQLx exactly and uses a local `[patch.crates-io]` entry.

`vendor/sqlx-sqlite/` contains all 56 published crate files. Only the normalized
`Cargo.toml` dependency declaration changes from `0.30.1` to `=0.39.1`; Rust files,
features, original workspace manifest, VCS metadata and both licenses stay byte
for byte identical. The application retains `#![forbid(unsafe_code)]`; no upstream
FFI block is added or edited. The original `.crate` archive is retained alongside
the copy, with the official registry SHA256:

`c2d12fe70b2c1b4401038055f90f151b78208de1f9f89a7dbfd41587a10c3eea`

The locked 0.39.1 binding checksum is:

`14a5f01b87b92705d07b433c06f96cbc764637d81a0bffb5734ce17cf0f9a2b3`

These identities can be checked against the primary
[SQLx driver index](https://github.com/rust-lang/crates.io-index/blob/master/sq/lx/sqlx-sqlite)
and [native binding index](https://github.com/rust-lang/crates.io-index/blob/master/li/bs/libsqlite3-sys).
`Cargo.toml.orig` deliberately remains the upstream historical workspace file;
Cargo builds from the normalized patched `Cargo.toml`.

The offline `deploy/scripts/verify-sqlite-vendor.py` verifies the archive digest,
complete file set, exact permitted difference, application's MSRV/driver patch
and lockfile's unique local driver plus published binding checksum. Source edits,
missing/extra files, symlinks, license changes and changed dependency bindings fail.
CI runs it before compilation/audit, and both package builders run it on their
staged payload. The CyberHerd archive verifier also checks the embedded provenance.
Review archives retain the original archive, source, licenses and verifier.

## Verification and storage boundaries

The native engine regression executes `SELECT sqlite_version()` through SQLx and
requires exactly 3.54.1. This checks the compiled executable, rather than trusting
only the manifest. The full locked financial/gateway/payout suites must also pass:
ordinary settlement, retained paid rows, migration, actual WAL/FULL/foreign-key
settings, original feeder UUIDs, signed outbox bytes and cross-journal outcomes
remain their existing contracts. No application schema/migration changes are
part of this dependency update, and no state is rewritten by preparing it.

Required checks include:

```sh
python3 deploy/scripts/verify-sqlite-vendor.py
python3 -B -m unittest discover -s deploy/tests -p test_sqlite_vendor.py -v
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
cargo tree --locked -i rsa --all-features
cargo audit --ignore RUSTSEC-2023-0071
```

The existing RSA exception remains conditional on its absence from the active
graph. No SQLite waiver is added. Results belong in an exact-source checkpoint
after completion; the dependency change alone does not prove passing tests.

This covers Rust binaries built with the reviewed bundle. Python's system SQLite,
other programs and an already installed household/VPS binary need their own actual
engine qualification; a green Rust audit does not update them. In particular,
whole-stack recovery must not assume that a system Python parser uses this bundle.
The legacy two-store restore helper still refuses CyberHerd schema.

The source change does not install a binary, load keys, enable CyberHerd, authorize
a financial review or operate the feeder. Production installation must preserve
all paid journals and follow the existing quiesced backup/qualification process.
Remove the local patch when a reviewed compatible upstream release supports the
fixed binding, with the same provenance, native-engine, audit and recovery checks.
