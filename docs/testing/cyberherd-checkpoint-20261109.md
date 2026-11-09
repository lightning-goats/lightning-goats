# CyberHerd source checkpoint — 2026-11-09

This is an incomplete library/presentation checkpoint on
`feat/cyberherd-20261109`, based on restoration commit
`51f330b`. The [implementation plan](../planning/cyberherd-implementation-plan.md)
retains the complete port scope and remaining work. No CyberHerd worker, spending
credential, membership ingestion or payout service is installed/activated here.

## Verified source behavior

- Exact points and six-decimal largest-remainder shares; zero member allocation
  remains zero. Deterministic amount/pubkey displacement, strict paid headbutts,
  no reaction headbutts, pre-join social evidence and no free re-entry using an
  already awarded type. NIP-05 is not a membership condition.
- Separate WAL/FULL storage, serialized transactional state/event/source receipts,
  concurrent duplicate handling, conflicting source rejection, restart/reset replay
  protection, owner isolation, deferred pause recovery and minimal stale discards.
- Operator-export wording: 50 categories/653 variants. Safe simple placeholders,
  deterministic variety, no forced goat-name suffix. The source renderer uses
  the original payment/feed/information pools; the selected variant controls naming.
- Idempotent audited template edits; removed variants and disabled categories stay
  removed/disabled after seed retries and reopening the database. Missing optional
  pause/invitation categories do not silently gain defaults. Weather/information
  audience remains overlay-only, and the payment message minimum remains 10 sats.

## Checks at checkpoint publication

- `cargo fmt --all --check`: passed.
- `cargo clippy --locked --all-targets --all-features -- -D warnings`: passed.
- Corrected unit-test binary: all 20 `cyberherd::` tests passed; all 12 matching
  presentation/accounting tests passed. This includes rendering every imported
  template, checking every payment variant's optional naming, and durable-store
  restart/concurrency/opt-out scenarios.
- `cargo test --locked --all-features`: running at checkpoint publication; do not
  interpret the focused results as a completed whole-repository regression run.
  The obsolete pre-correction run was stopped to release the build directory.
- `cargo tree -i rsa --locked`: no active reverse dependency.
- `cargo audit --ignore RUSTSEC-2023-0071`: failed on the already documented
  `RUSTSEC-2026-0459` in locked `libsqlite3-sys 0.30.1`. Dependencies/lockfile remain
  unchanged; no additional ignore was added. See the
  [SQLite assessment](../security/sqlite-wal-advisory-20261109.md). CyberHerd uses
  service-owned database paths and has no untrusted database/WAL upload/restore API.
- Staged source/template scan: no private-key blocks, nsec credentials or the
  previously disclosed temporary Strike key found. Local credential setup scripts
  and host secrets are excluded from this checkpoint.

Tests use isolated temporary databases; no Lightning payment, operator credit,
public Nostr message or physical feeding was generated for this checkpoint.
