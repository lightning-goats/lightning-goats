# New SQLite audit finding — 2026-11-09

Status: audit failure recorded; no exception added or dependency change made.
The Nostr/overlay candidate is not installed while disposition is pending.

The fresh documented gate reported `RUSTSEC-2026-0459` / `CVE-2026-119375` in
locked `libsqlite3-sys 0.30.1`. The unreachable-RSA check still reports no active
reverse dependency. Cargo.lock and SQLite dependencies are unchanged by this
Nostr/overlay work; the previous audit used an older advisory database.

[RustSec's advisory](https://github.com/RustSec/advisory-db/blob/main/crates/libsqlite3-sys/RUSTSEC-2026-0459.md)
describes disclosure of uninitialized process memory when opening an
attacker-written database and sibling WAL with inconsistent page sizes. It
requires the victim process to open that local file pair. The patched bundled
SQLite begins with `libsqlite3-sys 0.39.0` / SQLite 3.54.0.

Actual production readback found the state directory mode 0700, and database,
WAL and SHM mode 0600, all owned by non-admin runtime UID/GID 985, with no symlinks.
The public payment/overlay routes do not accept database/WAL files or database
paths. These constraints limit the advertised attack condition; they do not
patch SQLite or certify arbitrary imported backups. Opening an untrusted local
snapshot remains relevant, and runtime/administrator compromise is separate.

Official registry inspection found SQLx 0.8.6 is still the latest 0.8 patch;
its SQLite adapter requires `libsqlite3-sys ^0.30.1`, so a direct update to 0.39
cannot satisfy the existing graph. Published SQLx 0.9.0 requires Rust 1.94 and
still caps the SQLite binding below 0.38, so that major upgrade alone does not
fix this advisory. The repository/compiler remain Rust 1.88. No opportunistic
toolchain/financial-adapter migration or ad-hoc SQLite fork was introduced.

A bounded temporary exception, if approved by the operator, must explicitly
name this advisory and the current private, operator-managed database conditions.
It must not be represented as a clean unqualified audit or applied to untrusted
restore inputs. Otherwise keep the native candidate held and qualify an upstream
compatible SQLite correction/backport separately with ledger/recovery tests.
