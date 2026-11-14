# SQLx SQLite binding backport

`sqlx-sqlite/` is the published `sqlx-sqlite 0.8.6` crate, with **one manifest
change**: `[dependencies.libsqlite3-sys].version` changes from `0.30.1` to
`=0.39.1`. Its Rust sources, features, original workspace manifest, version/VCS
metadata and MIT/Apache licenses are retained byte for byte. No upstream unsafe
block is added or edited; application `#![forbid(unsafe_code)]` remains intact.

The original registry archive is retained in `upstream/`. Its SHA256, also
published in the official crates.io index, is:

`c2d12fe70b2c1b4401038055f90f151b78208de1f9f89a7dbfd41587a10c3eea`

`deploy/scripts/verify-sqlite-vendor.py` checks that archive's exact digest,
every original driver file and the single permitted manifest difference. It
also checks the application's patch/pin and the lockfile's unique local driver
and published native binding checksum. It requires no network or build and is
run in CI and before packaging. Review archives retain this provenance and both
upstream licenses. This directory is a dependency, not an application workspace.

The backport addresses [RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459.html).
`libsqlite3-sys 0.39.1` bundles SQLite 3.54.1 and supports Rust 1.88. Released
SQLx 0.8.6 restricts its binding to 0.30.x; released 0.9.0 requires Rust 1.94
and also predates the fixed binding. The rest of the SQLx graph stays at 0.8.6.
No audit exception or system-library substitution is used.

Remove this patch when a reviewed, compatible upstream SQLx release supports the
fixed binding. Re-run the native engine/version, durable accounting/recovery,
locked Rust, audit and release checks before changing the driver or engine.
