# Existing owner v2 candidate

## Selected Design And Constraints

Operator selected Option 2 for repository implementation and harmless testing.
Production HOLD; no owner replacement or home activation is authorized. The
source-derived candidate is `deploy/openhab/feeder-owner-v2.js`.

## Source Revision And Drift Check

Initial base 74d4333d8b89179f68cf3fb004230d6c256ddb69; fetched GitHub and
fast-forwarded onto reviewed integration merge
bf8749645c6e6942643a3b821cbb8804e7e200a8 (PR56), preserving the merged
Nostr work. Operator-exported owner source
SHA256 730053e0f3245cb83461e3fe6e3b05d49c8b508631e8cdb4a889c8be8d915978.
This is a snapshot, not a fresh protected JSONDB inspection. Recheck the live
rule digest before migration. No raw private source or credentials are committed.

## Affected Components

Existing OpenHAB rule, bounded JDBC ledger and result Item. Candidate requests
and individual entries carry `version: feeder-request-v2`; envelope is
`feeder-request-ledger/v2`. Existing gateway remains v1 and will reject v2.
No shipped owner selection or gateway behavior changes in this increment.

## Ordered Work Packages

1. Source-derived owner candidate and command-count mocks (this increment).
2. Actual OpenHAB 5.2.1/Java concurrency, timer lifecycle and JDBC restore tests
   with harmless Items; compare against the exported v1 negative control.
3. Explicit opt-in gateway v2 parser and bounded history recovery; real daemon
   through real gateway through harmless owner, including seeded 2340 -> 340.
4. Disabled-feeder migration/restore rehearsal, separately approved activation.

## Compatibility And Migration

This candidate is deliberately not a drop-in live rule. It rejects uncorrelated
legacy triggers, v1 requests, old ledger entries and missing restore. Initialize
an empty v2 ledger only through a reviewed migration that adjudicates prior
requests with feeding disabled. Do not relabel v1 receipts. It stops at 32
entries rather than evicting idempotency keys; long-running retention remains an
open compatibility gate, not an approved production behavior change.

## Tactical Protections During Migration

Keep gateway unresolved reservations and same-UUID no-resend behavior. Admission
is owned by a Java AtomicReference acquired before ledger reads; a Java identity
token spans the timer. Unknown restore, unfinished entries or accepted-write
uncertainty never authorize ON. Completion persistence cannot invoke a failed
ledger writer. Notifications are best effort. No retry increments counters.

## Tests And Security Validation

Run `node --test deploy/openhab/tests/owner.test.cjs`. Tests use separate VM
invocations and a modeled Java atomic primitive, with actuator command counts.
They cover overlap, reentrant admission, duplicates, notification loss, accepted
write failure, complete readback loss, restart, OFF failure, missing/v1 restore
and cooldown. This does not prove actual JVM concurrency or JDBC durability.
Actual cache semantics source: OpenHAB core 5.2.1
`CacheScriptExtension.TrackingValueCacheImpl.get(key, supplier)` holds cacheLock
around computeIfAbsent. Cache unload removes keys when no accessors remain.
The callback checks current guard identity and fails closed on replacement.
See https://github.com/openhab/openhab-core/blob/5.2.1/bundles/org.openhab.core.automation.module.script.rulesupport/src/main/java/org/openhab/core/automation/module/script/rulesupport/internal/CacheScriptExtension.java

## Performance And Resource Benchmarks

Keep 2048-character request, 8192-byte ledger, 32-entry limits, five-second
cooldown, one-second pulse and 20 x 50ms readback budget. No polling after that
budget. Real scheduler jitter, concurrent admission latency and heap measurements
remain outstanding; Node mocks are not runtime performance evidence.

## Rollout And Rollback

Do not install this candidate. Preserve original rule and both ledgers, disable
feeding, review compatible readers and reconcile unresolved requests before any
migration. Rollback must preserve v2 receipts and holds; do not install v1 over
v2 state or reset the ledger to clear uncertainty.

## Acceptance Criteria

Atomic ownership and durable acceptance before any ON; at most one ON per UUID;
no distinct admission while unresolved; completion never downgraded by delivery
or persistence failure; crash/restore tests; compatible retention and bounded
history; real daemon/gateway accounting proof. All parent gates remain OPEN.

## Open Decisions

Production retention without UUID eviction, explicit legacy-trigger migration,
JDBC restore guarantees, and authorized harmless-runtime deployment remain
unresolved. This increment is a review candidate, not a verified live fix.

## Candidate review evidence

Ten Node mock regressions passed on this VPS. Independent read-only review
found insufficient space reserved for the terminal timestamp; admission now
budgets completion space before persistence or ON, with a regression covering
long IDs near the 8192-byte limit. No live OpenHAB or database connection was
used. Verification remains blocked on the actual-runtime acceptance above.
