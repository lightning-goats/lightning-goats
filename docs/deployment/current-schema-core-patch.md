# Current-schema core program update

This is a separate, explicitly tagged mode of the authenticated upgrade tool.
It preserves the existing overlay-only CyberHerd stage and any existing private
contact configuration. It does not activate a new feature, change credentials,
modify units or financial budgets, or authorize schema migration or restore.
The original schema-23-to-24 contact procedure remains separate and rejects these
records. Never reuse a completed transaction or run checkout code as root.

After qualifying exact source and independently pinned core/review archives,
install a fresh root-owned tool closure using the documented authenticated
launcher. Verify the launcher hash externally before loading it, then verify its
full closure. The launcher selectors are:

```text
--current-core-prepare ARCHIVE REVIEW_ARCHIVE SOURCE ARCHIVE_SHA REVIEW_SHA REVIEW_DIR
--current-core-maintenance plan REVIEW_DIR EXECUTION_DIR --review-sha256 PREPARED_SHA
--current-core-maintenance apply EXECUTION_DIR --exclusive-maintenance
--current-core-maintenance resume EXECUTION_DIR --approve-resumption
```

These are argument shapes, not unreviewed production commands. Select new direct
children of the protected upgrade base. Preparation and plan open no journals,
stop no services, and freeze installed files, original UIDs, live PIDs, effective
units, masks, source archive hashes and exactly three selected program hashes.
Preparation exercises only the candidate CLI help under the original UID in a
new network namespace. The program targets are `lightning-goatsd`,
`lightning-goatsctl` and `cyberherd-statectl`.

Apply requires the unchanged frozen installation, the shared exclusive maintenance
lock and explicit maintenance authorization. It stops and persistently masks all
four journal writers. The original installed native verifier creates and verifies
a current backup of all four journals. The selected new native verifier must
accept that same bundle against its exact compiled migrations and checksums
before a candidate CLI can open even a copied database. Unknown/older/newer
schemas fail with programs unchanged and services stopped for inspection.

The original and selected CLIs then run `status` on a separate UID-owned core
copy, without capabilities, supplementary groups, credentials or networking.
Their nonempty status output must match exactly. This validates the actual
retained configuration with native code while preserving live main/WAL/SHM
bytes. The existing durable program transaction replaces only the three sealed
programs. Live database bytes must remain exactly unchanged throughout apply;
no live CLI, migration, database replacement or restore runs in this mode.

Resume revalidates both native backup receipts, the accepted backup manifest,
selected programs, original configuration/identities/masks and exact stopped
database bytes before restoring the original core unit and removing only its own
membership mask. It starts the existing core and member once and verifies health.
Both outgoing roles remain masked. The membership unit's existing boot enablement
is preserved. An ambiguous or interrupted phase is retained for inspection;
there is no automatic retry, stale-state rollback or implicit old-binary fallback.

Protected-file regressions cover cross-mode/numeric-flag refusal, retained verifier
drift, candidate schema rejection before copied-store opening, differing native
status, unchanged financial/configuration bytes, and paid-image drift before
resume. Native/systemd behavior is a separate guarded disposable CI exercise with
synthetic credentials, pending feed/signed history, actual installed identities
and override ON. Its current-schema phase uses the same build as both installed
and selected programs; different-byte replacement is covered by the protected
file fixture. Neither establishes actual production deployment or transfers.
