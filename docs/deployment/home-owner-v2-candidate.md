# HOME owner-v2 correction candidate

Adopts only the owner implementation and fixtures from PR57
`5cd7976f78e747eaf7bc3cc130199353a891191b`, based on main
`df34bcdea5a178bb72193d1a431ea3a9f3e0c9f1`. The existing physical owner remains
unchanged. This candidate has only been installed in a separate unlinked test
fixture. It is not a replacement approval or production-ready owner.

## Implemented and tested

`deploy/openhab/feeder-owner-v2.js` separates command ingress
`GoatFeeder_ManualRequest` from a dedicated unlinked receipt String,
`GoatFeeder_OwnerLedgerV2`. The ledger must have autoupdate disabled and must not
receive commands. Ownership is acquired before mutable ledger reads. Admission
waits for the exact asynchronous Item update and then exact JDBC readback before
ON. Complete receipts are immutable even when notification fails. An unresolved
admission or ambiguous persistence holds subsequent work; no automatic retry.

The five-second interval uses the actual start time in memory and the latest
completion timestamp across retained receipts after restore. Completion is a
conservative bound later than the prior ON, so slower persistence cannot shorten
the interval. Complete records require a valid nonregressing completion time.

Eighteen JavaScript tests pass. Added regressions reproduce the original command
prediction clobber and the reviewer's slow-persistence timing failure before the
fix. They cover delayed Item/JDBC work, process loss after admission, lost result
notification, restored receipt replay, and both cached and restored cooldown.
The capacity test completes 32 distinct requests, refuses the 33rd and still
recognizes the oldest UUID after restart. No unresolved receipt is aged out.

**Capacity remains bounded at 32 entries / 8192 bytes and fails closed.** This is
a safety regression test beyond the boundary, not sustained-operation acceptance.
No compaction, silent eviction, store reset or claimed anti-rollback marker was
added. Full-host rollback protection requires independently recoverable history
or a physically disabled reconciliation gate, as narrowed by the project lead.

## Repeatable harmless runtime fixture

```sh
node --test deploy/openhab/tests/owner-v2.test.cjs
python3 -B -m unittest discover -s deploy/tests -p test_owner_v2_fixture.py -v
python3 deploy/scripts/prepare-owner-v2-fixture.py --provisioning-env /protected/env
# Only after reviewing the fresh-only plan:
python3 deploy/scripts/prepare-owner-v2-fixture.py --provisioning-env /protected/env --apply
python3 deploy/scripts/check-owner-v2-fixture.py --provisioning-env /protected/env
python3 deploy/scripts/check-owner-v2-fixture.py --provisioning-env /protected/env --apply
```

The preparer refuses existing fixture Items/rules/consumers/links. It rewrites all
physical bindings and cache keys to `LightningGoatsOwnerV2Test*`, creates seven
ungrouped, unlinked Items and three exact-readback rules, and bootstraps only the
fixture ledger. It does not delete prior evidence or replace the physical owner.
The checker revalidates bindings, channels, consumer rules and ledger metadata,
sends one new synthetic UUID twice, and checks every ON-command delivery, counter,
OFF state, exact complete ledger and an explicit bounded JDBC query. A failed
check stops; it never automatically retries with a fresh UUID. For interrupted
verification, `--resume-request-id UUID --apply` requires that exact completed
receipt and performs only its duplicate replay. Retained evidence is not cleared.

On 2026-09-13 the corrected rendered fixture SHA-256 was
`4f02d5ad19635c9842c83256cdb024107e38547eb8bd58eb7796960ed3b23053`.
Real OpenHAB/JDBC execution produced one unlinked ON, counter 1, actuator OFF and
a durable complete receipt. The first REST evidence check exposed zero-based
JDBC pagination; page 1 was empty. The checker was corrected to page 0, then
resumed verification using only the already completed UUID. Its duplicate produced
zero additional ON commands and preserved the exact complete receipt. This tests
real Graal/JDBC completion and duplicate handling, not an OpenHAB service restart,
crash injection or cross-host acceptance. Local evidence is retained at
`/tmp/lg-owner-v2-live-fixture-check.json`; no credential appears in that report.

## Remaining contract and activation gates

HOME requested VPS coordination in #17 comment5654654270 before editing shared
`src/openhab.rs`. No such file or `tests/gateway_admission.rs` is changed here.
The currently installed gateway remains v1/UUID-canary only; this candidate's
`feeder-request-v2` envelope is not yet a compatible production adapter.

Lost-result recovery must consult durable completion evidence without sending a
new actuation. A current ledger Item can precede its JDBC commit and is not alone
authoritative. The gateway/owner history and refusal tombstones need coordinated
retention, restart and restore acceptance, including full-host rollback. Do not
infer durable history completeness from one JDBC page or its datapoints count.

Before any physical replacement, pin and independently review the complete
adapter/owner migration, preserve existing receipts/metadata/OFF backstops, prove
real restart and serialization behavior, and obtain the explicit operator step.
Disabling this fixture stops only its three rules; preserve its Items and JDBC
history. Do not run a generic cleanup against production owner state. The VPS
agent is the intended independent exact-commit reviewer. #17/#15/#16 remain open.
