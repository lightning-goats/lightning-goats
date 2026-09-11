# JDBC export and completion-finality gap

Production remains **HOLD**. This follow-up adds offline inspection and regression
evidence, not automatic history-based confirmation or a physical-owner change.

## Observed source and sanitized fixture

The operator saved four existing JDBC records in
`/home/sat/lg-owner-jdbc-history.json`, owner sat, mode 0600. The read-only remote
metadata check confirmed 13103 bytes and SHA-256
`9b7cc2886a3b53e61f66dbdb54e959965476052873abb72f05b09637c46e220a`.
The response has `name`, a decimal-string `datapoints`, and `data` rows containing
integer millisecond `time` and JSON-string `state`. Each state is a
`feeder-request-ledger/v1` snapshot. The leading identity appears accepted in
the first two rows and complete in the last two; each snapshot has 19 entries.

Automatic approval review rejected transferring the private history to the VPS.
The approved narrower inspection ran on the home host. It replaced every request
ID and every timestamp before returning a fixture. No original IDs, timestamps,
credentials or raw history were retained locally. The committed fixture preserves
schema, repeated identities and statuses, **not real timing intervals**:
`tests/fixtures/openhab/jdbc-history-sanitized.json`.

The operator confirmed this GET (line wrapping removed):

```text
/rest/persistence/items/GoatFeeder_ManualRequest?serviceId=jdbc&starttime=1970-01-01T00%3A00%3A00.000Z&boundary=false&itemState=false
```

`endtime`, `displayState`, `page` and `pagelength` were omitted. Current-state and
boundary injection were disabled. In the version-pinned implementation, epoch-zero
start time selects a one-day default window, not all history; omitted page length
requests an effectively unlimited result within that window. The four rows do
not prove all-history coverage. Preserve that limitation in any derived evidence.

## Why the four rows cannot authorize automatic recovery

The inspected owner script (`730053e0f3245cb83461e3fe6e3b05d49c8b508631e8cdb4a889c8be8d915978`)
can persist a complete row and then write `failed/execution_error` when either
the readback or result publication throws. It can also fail to persist that later
failure. The gateway could read complete in the middle of this sequence. Two
identical complete rows, a short delay, or an apparently latest complete row do
not prove finality. A newer failure must never be bypassed by searching backwards
for success.

A successor entry is not an unconditional witness either: the current owner
reads the ledger before acquiring its busy token, so a concurrent invocation can
hold an earlier snapshot. Resolving this requires a reviewed authority/finality
contract, not a broader success parser or a longer polling delay.

The candidate offline tool validates bounded bodies, rows, ledger bytes/entries,
unique JSON keys and identities, version/Item identity, timestamps and ordering.
It reports the latest matching observation but **always** reports
`completion_is_authoritative=false` and `release_reservation=false`. It performs
no network request, command, database write or runtime reconciliation.

```sh
python3 deploy/scripts/inspect-owner-history.py \
  tests/fixtures/openhab/jdbc-history-sanitized.json \
  00000000-0000-0000-0000-000000000001
python3 -m unittest discover -s deploy/tests -p test_owner_history.py -v
```

Seven regression groups cover the sanitized observations, complete-then-failed,
truncated/absent history, wrong schema/count/version/identity, time contradictions,
invalid offset normalization, and body/ledger/duplicate-key limits. These are offline data tests, not physical
tests or proof that the current owner completes safely under every failure.

## Version-pinned REST contract

[OpenHAB 5.2.1 PersistenceResource](https://raw.githubusercontent.com/openhab/openhab-core/5.2.1/bundles/org.openhab.core.io.rest.core/src/main/java/org/openhab/core/io/rest/core/internal/persistence/PersistenceResource.java)
defines USER-accessible GET `/rest/persistence/items/{itemName}`. A future reviewed
reader must explicitly select JDBC, disable `itemState`, `boundary` and
`displayState`, and impose nonzero bounded pagination. Main query ordering is
ascending; `datapoints` counts returned rows, not total history. Current-state and
boundary options can add synthetic rows. Do not infer latest-history completeness
from the first page or enable unlimited page length.

## Next authority review

Before enabling automatic lost-result recovery, prepare and review a physical-owner
contract in which a durable terminal receipt cannot later be downgraded by result
publication/readback handling, and admission reads are serialized with ownership.
Regression criteria must include a pause after complete persistence, readback
timeout, lost result update, failed correction persistence, concurrent owner
invocations, restart and history truncation. Until that contract is accepted and
separately approved for the home host, PR #49's same-UUID polling and permanent
unresolved reservation remain authoritative. No live owner modification is
authorized or performed by this evidence change.
