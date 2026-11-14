# Native reward backlog review — 2026-11-11

The native `cyberherd-statectl review-rewards` command is qualified in source,
with protected-root and installed CLI evidence. A separately installed reader
also accepted an existing historical backup on the VPS. This does not activate
rewards/payouts, change a live journal or establish the current financial backlog.
The full twelve-part port remains incomplete.

## Source and CI

Qualified source `b0125c8b7ee513adb9481f71b7e3e3a63ef94240` passes all ten workflows:
[CI](https://github.com/lightning-goats/lightning-goats/actions/runs/43015909340),
[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/43015909161),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/43015909352),
[real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/43015909160)
and six home/network workflows. Tested merge
`80401adf8c51fbadd7c0f1eccc71e3c662503b92` independently compares with zero changed
files. Format and strict locked all-target/all-feature Clippy pass. The ordinary
native suite passes 526 tests with five explicit ignores; required separate
positive root ownership/snapshot/bootstrap/upgrade fixtures also pass. Deployment
passes all 119 protected-root Python cases and 41 browser cases; the 314 general
Python cases retain 85 environment skips.

Four new native cases cover all six barrier/plan categories, frozen versus current
allocation, a lost reservation acknowledgment, pending intake and core cursor lag;
complete classification with only 32 retained examples; contradictory funding,
oversized snapshots and altered plan hashes; and withheld private JSON values in
decode errors. The required root four-journal fixture checks report permissions,
backup preservation, refusal of reused/nested destinations and review of accepted
copies after the stopped-writer condition is no longer asserted.

Installed systemd job `130508573976` invokes the actual packaged CLI on its
accepted schema-24 backup. It verifies the exact backup binding, full barrier
examination, retained unresolved physical work, private output and unchanged
manifest/image bytes. The existing synthetic stack then resumes normally.
Provider operations and gateway POSTs stay zero. Its report keeps
`activation_authorized=false`, `external_reconciliation_required=true` and
`live_host_qualified=false`.

Systemd artifact `13852328765` has verified ZIP SHA256
`42ff60c0350f659b01d458e2ffbbc9ea3aff7003ce616d63df369f06c2abd267`.
All six internal checksums and the exact positive native-review report were
independently checked. Review artifact `13851734866` has verified ZIP SHA256
`af4effd62e2f5bb84d20c6d7c9bd18fce6e45e00f312612c94761395e2271880`;
its archive checksum and all 104 internal payload checksums pass, with the exact
tested source and `activation=none` build metadata. No artifact program ran
during these data-only verification steps.

## Actual historical-copy acceptance

Only the qualified offline reader was copied to a new protected root-owned
location; existing installed application programs and tool generations remain.
The selected executable is 9,201,184 bytes, SHA256
`f6e4af5bbdc0f88bb26d8031dca2eaee21961d4a4529cd26ca850f70e5e318cf`.
External system checksum verification preceded execution. No writable checkout
code ran as root.

The reader ran with a scrubbed environment, a fresh isolated network namespace,
NoNewPrivileges and all capabilities removed. It verified and inspected only a
previously accepted backup and wrote its separate private review. Native SQLite
opened the copied images, including copied committed WAL where present; live
journals and credentials were not opened. Backup/report paths, record counts,
UUID examples and operational findings remain private.

Independent readback confirms the report's exact accepted-manifest hash/ID,
policy scope, complete barrier counts and bounded-example accounting; the
accepted manifest and every declared image still match their original bytes.
Report privacy and refusal of activation remain intact. Original managed
application file fingerprints and service states also remain unchanged. No
application replacement, service restart, migration, payment, public event or
physical command occurred.

This is acceptance of the reader against **historical copied state**. It does
not establish current funds, present recipient eligibility, full provider history
or a safe activation cutoff. A later financial maintenance transaction must
select its fresh verified stopped-writer backup and bind the review to that
exact image set. It must separately preserve unresolved work and satisfy the
operator/provider requirements. The report never closes a plan, creates a
reservation, clears recovery markers or authorizes resumption.

See [the command and private report contract](../deployment/cyberherd-state-maintenance.md#review-retained-reward-backlog)
and [financial candidate preparation](../deployment/cyberherd-financial-stage.md).
