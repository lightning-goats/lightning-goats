# Inspected feeder-request-v1 owner contract

Production remains **HOLD**. The operator prepared a read-only export of rule
`88bd9ec4de` on home host `10.8.0.6`; the gateway's runtime authority was not
installed or expanded and no physical command was sent.

## Provenance

The operator independently confirmed home RSA host fingerprint
`SHA256:sgPPLGHwCyEk5Zuebx2s2F/1IvvQmFd750+AVEM9NL8`. Strict pinned SSH read only
`/home/sat/lg-owner-review-88bd9ec4de.json`, mode 0600, uid 1000, 14866 bytes.
Export SHA-256: `025e3a801ebbd5dfc016351473185a4a8bf0d6db91c0c84dd1bcf60858888a47`.
Script SHA-256: `730053e0f3245cb83461e3fe6e3b05d49c8b508631e8cdb4a889c8be8d915978`,
matching the historical audited owner. The host reports OpenHAB 5.2.1-1.
The source was scanned before private transfer; token markers referred only to
dynamic invocation ownership variables, not credentials.

## Request and result

Trigger: `core.ItemCommandTrigger` on `GoatFeeder_ManualRequest`.
The typed `feeder_request_v1` adapter sends exactly:

```json
{"requestId":"00000000-0000-4000-8000-000000000001","requestedAt":"2026-09-11T00:00:00.000Z"}
```

This example is illustrative; the gateway supplies the admitted UUID and fresh
UTC request time. Owner limits: 2048 command characters, request age at most
120 seconds, future clock skew at most 30 seconds, owner cooldown 5 seconds.
The gateway's existing durable reservation precedes HTTP dispatch. Neither a
successful HTTP response nor request receipt confirms actuation.

Result Item: `GoatFeeder_ManualResult`. Exact schema:

```json
{"requestId":"00000000-0000-4000-8000-000000000001","status":"complete","reason":"complete","at":"2026-09-11T00:00:01Z"}
```

| Source status/reason | Gateway interpretation |
| --- | --- |
| `accepted/accepted` | Pending; no completed actuation |
| `running/pulse_started` | Pending; emitted even before ON |
| `complete/complete` | Exact UUID owner completion receipt |
| `denied/busy`, `denied/cooldown`, `denied/request_stale` | Invocation rejected, overall request remains unresolved |
| `denied/duplicate` or ledger-related denial | Unresolved; not proof of no earlier actuation |
| `failed/execution_error`, persistence failure or `restart_uncertain` | Ambiguous; retain reservation |
| Missing, unrelated, malformed, aliases or contradictory fields | Never confirm or release reservation |

The completion sequence commands OFF, increments and reads back `GoatFeedings`,
writes/readbacks the JDBC ledger, then emits the result. It is owner-sequence
confirmation, not a physical sensor measurement of food dispensed. Unknown
properties, statuses and status/reason combinations fail closed. No generic
`success=true`, bare UUID or `completed` alias is accepted by this protocol.

The explicit `uuid_canary` protocol preserves harmless echo fixtures and is
restricted to `LightningGoatsCanaryRequest` / `LightningGoatsCanaryAck`. It cannot
bind the physical request/result Items. Legacy `request_payload_template` config
is rejected; migrate reviewed configurations to an explicit protocol.

## Durable recovery gap — acceptance remains open

Follow-up: the operator supplied a four-record JDBC response. See
[openhab-jdbc-recovery-evidence.md](openhab-jdbc-recovery-evidence.md) for the
sanitized fixture and the source-backed complete-to-failed finality race. The
export's existence does not close automatic recovery acceptance.

The request Item also holds a `feeder-request-ledger/v1` ledger with at most 32
entries and 8192 UTF-8 bytes. On restart, interrupted accepted/running entries
become `failed/restart_uncertain`. Duplicate UUID commands are denied; the gateway
must still retain permanent UUID state because the owner's ledger is bounded.

**Do not infer persisted completion from a current Item-state snapshot alone.**
`writeLedger` posts the Item update before persistence/readback completes; failure
can replace an apparently complete snapshot with `failed/execution_error`.
If the transient result was missed or overwritten before gateway persistence,
the current adapter keeps the request unresolved and polls the same UUID without
resending. A validated read-only authoritative persistence-history contract and
sanitized response fixtures are still needed to recover those completions safely.
No persistence REST shape or fallback is guessed in this change.

Likewise, owner denials are not gateway pre-dispatch tombstones. Even busy,
cooldown or stale denial describes only that invocation; absence from bounded
history does not establish that a UUID never actuated. Mapping these denials to
the existing daemon `not_dispatched` outcome would authorize a fresh UUID and
could duplicate physical action. Existing gateway pre-dispatch safety/capacity
refusals keep their proven recovery behavior.

## Verification scope

Unit fixtures and loopback mock responses derive from the inspected source;
they are not captured physical-test results. Tests reject generic success aliases
and contradictory outcomes, check fresh exact request JSON, exercise progress,
denials/failure, mismatched UUIDs and late completion after gateway restart, and
assert command counts. The shipped-examples real-daemon/real-gateway mock test
uses this typed owner protocol for 2340 sats, two confirmations and 340 remaining.

F04 remains partial until authoritative lost-notification recovery, live harmless
owner fixtures, safety Item/token permissions and final staging acceptance are
verified. Preserve F09 containment and the parent acceptance gates. No change to
the physical owner is proposed here.

Local verification on the new Fedora VPS: focused OpenHAB tests passed (7),
real daemon/gateway boundary tests passed (8, 39.64 seconds), deployment tests
passed (36), formatting and locked all-target/all-feature Clippy passed. An
independent read-only candidate review found no concrete bypass/regression.
The complete locked suite and exact-head Security/Deployment artifacts results
are recorded in the draft PR; do not infer those results from focused checks.
