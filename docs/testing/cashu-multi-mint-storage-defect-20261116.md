# Confirmed shared-CDK-store recovery defect

The new actual-CDK regression uses two independent issuers and one application
journal/writer. It loses the first issuer's swap reply, acquires from the second,
then restarts with acquisition disabled and attempts original recovery.

The distinct-secret control passes. Using the same input secret at both issuers
fails with `mint 0 recovery: pending rather than acquired`; the second issuer
acquires successfully. The application correctly distinguishes the two mint/proof
identities, but CDK SQL storage keys proof rows globally by `y` and its
`update_proofs` replaces all fields on `ON CONFLICT(y)`. The source is the pinned
`cdk-sql-common` 0.18.3 wallet schema and implementation. CDK saga recovery itself
filters by mint/unit, so changing only the recovery selection does not address
the proof-storage boundary.

Retain the equal-secret regression and its distinct-secret control; the original
shared-store failure is the baseline for the correction.
Required correction: durable mint-scoped CDK storage under the same wallet seed,
application journal and serialized writer authority. Acquisition, melt/split,
recovery, status and backup inventory must select the same retained namespace.
Do not rewrite proof secrets or redefine proof identity to hide the collision.
Existing shared-store custody and pending material must remain retained and
fenced until explicitly migrated/reconciled; do not initialize empty per-mint
stores and silently ignore old holdings. Adding/removing configured acceptance
must not destroy namespace bindings or recovery material.

This defect is development-only evidence, not a live financial incident. Cashu
has not been installed or financially enabled. Earlier passing checkpoints remain
scoped to their source/artifact hashes and did not cover this equal-secret case.

The mint-scoped adapter now passes both equal-secret and distinct-secret
recovery cases. Runtime acquisition and treasury execution/recovery select the
same registered per-mint store under one seed, journal and writer. Namespace
bindings are retained before initialization and marked ready only after CDK
opens successfully. Any incomplete namespace fences startup, even when absent
from current configured work. Legacy proof/saga custody and old journal work
refuse automatic adoption. All runtime wallet gates pass with 165 native tests
and seven ignored ([evidence](cashu-mint-storage-runtime-20261116.json)).

The subsequent read-only legacy startup ordering and private namespace health
patch pass formatting/strict Clippy; native qualification is still running.
Backup inventory now verifies every retained namespace and exact file binding,
with fourteen synthetic tests. Explicit coordinated migration/restore and
end-to-end multi-mint financial settlement remain incomplete. No live custody
has been migrated or enabled.
