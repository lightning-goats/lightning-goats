# New SQLite audit finding — 2026-11-09

Status: audit failure remains open and unignored; no dependency or workflow
exception introduced. The scoped Nostr/overlay update proceeds under the existing
[risk-tiered policy](../testing/phase1-verification-matrix.md), preserving the
verified private database conditions and without importing database/WAL files.

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

The operator has already authorized the bounded relay/presentation work. Its
enabled public path does not provide the attacker-written files described by
the advisory; no database parser, dependency, import or restore behavior changes.
The practical pilot policy blocks an affected unsafe path rather than every
unrelated change. Consequently this finding is retained separately while that
scoped update proceeds; no new approval or audit ignore is inferred or added.
This is not a clean audit result or proof that SQLite is patched. Do not apply
this assessment to untrusted restore inputs. A compatible upstream correction
or reviewed backport remains follow-up and needs ledger/recovery qualification.
