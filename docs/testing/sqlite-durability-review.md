# SQLite durability correction after integrated review

Production remains HOLD. This continuation starts from draft #40 head
`dc5a88c3371d62d2795b067df42bc005fe0de334`; the reviewed integration range starts
at main `80ae37c7b20950b345a1510fa16a05708925f8b8`. Earlier evidence remains valid
within its stated scope. This is an inherited daemon storage defect, not a
regression introduced by #40's systemd rehearsal.

## Reproduction and scope

`AppConfig::validate` previously accepted any `sqlite://` prefix, and
`LedgerStore::connect` requested WAL/FULL without checking effective storage.
The real operator CLI accepted `sqlite://:memory:`. The new direct-store
regression also accepted all 15 tested volatile representations: empty temporary
databases, encoded memory names/options, repeated schemes, duplicate mode
parameters, native SQLite `file:` URIs and memory VFS overrides. Payment, credit,
feed ambiguity, inbox and signed outbox history must survive process exit.
Configuration is operator-controlled; remote attacker control is not established.

Before the fix, the new config/direct-ledger/CLI tests failed and the ordinary
file control passed. The gateway's existing runtime check rejected the tested
memory VFS forms; those probes did **not** demonstrate a gateway runtime bypass.
Its configuration check still accepted them. Both stores now use the same
filesystem policy and runtime verification so their durability requirements
cannot drift. See `evidence/sqlite-durability-baseline-20260910.json`.

## Supported storage contract

Use an ordinary nonempty filesystem path following `sqlite://`, such as the
shipped absolute paths or `sqlite://relative.db`. SQLx percent decoding remains
supported for spaces, `?` and `#` in filenames. Optional query parameters are:

- `mode=rw` or `mode=rwc` (the existing create-if-missing behavior is preserved);
- `cache=private` or `cache=shared`;
- `immutable=false` or `immutable=0`.

Read-only, immutable, memory and custom VFS modes, empty paths, repeated schemes
and nested native `file:` URI filenames are rejected. Query keys and values use
the same form decoding as SQLx; every occurrence is checked, including duplicate
mode options whose flags SQLx retains. The direct `form_urlencoded` dependency
reuses the already locked version; no dependency version changes.

Before any migrations or gateway table creation, a connection must report a
nonempty main filename, effective `journal_mode=wal`, `synchronous=2` (FULL) and
enabled foreign keys. Preflight fails promptly; every new pool connection is
checked again. No unsafe configuration is silently converted to another mode.
The ledger retains its five-connection pool and gateway its one-connection pool.

This verifies SQLite's effective settings, not the durability of the underlying
host disk, filesystem or backups. Deployment must use the reviewed persistent
state directories and paired-store backup/restore procedure.

## Verification and acceptance

Run the focused config/direct-store/real-CLI tests with:

```sh
cargo test --locked --all-features --test sqlite_durability
cargo test --locked --all-features --lib sqlite::tests
cargo test --locked --all-features --lib gateway::store::tests
```

The positive control checks 2340 synthetic sats from another CLI process, then
reopens the store and checks payment deduplication, credit, event count and
overlay identity. Additional tests hold all five pool connections at once,
exercise actual memory connections and weakened PRAGMAs, and preserve encoded
filesystem paths and supported options. Existing real daemon/gateway/mock-owner
command-count, restart, settlement, signed outbox and restore tests remain
required, along with locked Rust, Security and Deployment workflows.

Keep the stack unmerged until the correction and integrated review pass; recheck
the final integration head after base-first merges. Merging code does not accept
the actual owner contract, website/browser, network containment or production
credentials. Keep #6/#15/#16 open. No real payment or physical feeding is part of
this storage correction.
