# Cashu collector handoff custody repair

The collector consumes the original wallet proofs when it swaps them into
collector-owned outputs. A wallet row left `UNSPENT` after that handoff can
overstate local spendable custody. The mint's NUT-07 result determines whether
the original proof is spent. Trace the accounting separately through the
wallet acquisition, collector input digest and output receipt, and immutable
core receipt and handoff acknowledgment. Keep operations with missing or
conflicting attribution held with their original identifiers.

The normal handoff records the exact original proofs as `SPENT` after both
acknowledgments agree and before the wallet marks delivery complete. The
original proof rows remain available for a retry of that same handoff.

## Rehearse on a verified copy

Use a root-private directory with protected ancestry, a standalone CDK
namespace image from a verified checkpoint, and native observations from the
same image. Save the image as `mint-copy.sqlite`. Keep every exact observation
page as `observation-0.json`, `observation-1.json`, and so on. The observation
files must cover the entire original non-spent candidate set for the selected
namespace. For a live repair, perform fresh observations while production
writers are fenced. Keep raw mint observations and reconciliation private.

Create a reviewed `REPAIR-PLAN.json` with the following fields.

| Field | Value |
| --- | --- |
| `version` | `1` |
| `checkpoint_id` | Verified checkpoint UUID |
| `observation_anchor_sha256` | Exact observation stage or source evidence anchor digest |
| `source_sha256` | Original standalone namespace image SHA-256 |
| `mint_id`, `mint_url` | Original namespace identity and issuer origin |
| `observation_sha256` | Ordered SHA-256 digests of the native observation files |
| `approved_points` | Exact reviewed public proof points for the custody-only repair |

Inspect the plan independently, compute its SHA-256 and use the reviewed
`lightning-goats-cashu-statectl` build.

```sh
lightning-goats-cashu-statectl prepare-custody-repair PRIVATE_COPY_DIRECTORY REVIEWED_PLAN_SHA256
```

The tool copies the original and updates only approved `UNSPENT` rows whose
mint observation is `SPENT`. It refuses an unapproved stale proof, incomplete
observation coverage, a mismatched namespace, a competing operation owner,
changed input bytes or unexpected proof triggers. The whole update is one local
SQLite transaction. The published `repaired/` directory contains the image and
receipt, and an exact retry verifies and reuses that result. Proof rows,
acquisition history and operation IDs are retained. No core, collector,
mint, payout or feeder journal is written by this command.

## Production handoff

Review the repair receipt and logical difference against the original, with
both hosts' writers fenced. Verify receipt, credit, collector, payout and feeder
accounting unchanged. Use an operator-approved, recoverable file transaction
to publish the exact reviewed image into the single matching namespace.
Preserve the stopped original state and permissions. Require an exact old or
new fingerprint before retry, no unreviewed SQLite sidecars or writer locks,
and a verified backup of the original bytes. A copy receipt grants no install
or resume authority. Retain it alongside the repair plan and mint evidence.

Collect the repaired paid state and supplemental journals together. Verify a
separate recoverable encrypted copy and refresh custody reconciliation across
all retained namespaces. Require zero mint-confirmed spent proofs still locally
listed as spendable. Preserve pending operations with their original IDs and
hold unresolved outcomes. Follow the sole-writer, admission and rollback
sequence in the [HOME cutover runbook](home-stateful-cutover.md).
