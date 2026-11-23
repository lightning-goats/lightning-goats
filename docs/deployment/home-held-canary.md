# Separate held-canary acceptance fixture

HOME implements this fixture for the VPS cross-host requirements acknowledged
in #17. The existing echo canary, its count/history and physical owner are not
changed. This is an unlinked acceptance fixture, not another physical owner.

## Exact control contract

Current generation uses the fixed `LightningGoatsHeldCanary2` prefix. Request,
Ack, Release and Journal are Strings; Count is Number; Hold, Fault, Override,
RemoteEnabled and Bootstrap are Switches. All ten Items are unlinked and
ungrouped. Journal/Count/Fault have autoupdate disabled. Two project-only rules
handle Request/Release and fresh persistence bootstrap. RemoteEnabled starts
OFF, Hold starts ON, Fault starts OFF. No deployed gateway is bound to these Items.

| Operation | Effect |
| --- | --- |
| UUID command to Request | Count every delivery; persist an ordered journal row before acknowledgement |
| Hold ON | Valid requests become held, with no matching acknowledgement |
| UUID command to Release | Release only that already-recorded UUID, persist it, then update Ack; Count does not change |
| Release replay | Preserve released timestamp/status; no new request or delivery |
| Hold OFF | Subsequent valid requests persist released state before acknowledging |
| Fault ON or persistence ambiguity | No new acknowledgement; no automatic reset |

The gateway-facing protocol remains a proposed `uuid_held_canary` opt-in for
fixed `LightningGoatsHeldCanary2Request`/`LightningGoatsHeldCanary2Ack` bindings.
This new allowlist requires VPS ACK before the shared adapter edit; it is **not
implemented by this fixture PR**. Existing `uuid_canary` behavior stays unchanged.
The public gateway request/status API needs no new control endpoint. VPS must
never receive an OpenHAB credential or directly invoke Release. HOME performs
release via its local CLI within the separately reviewed cross-host manifest.

Journal format is `held-canary/v1` with ordered `deliveries`: sequence,
requestId (null for malformed input), held/released/invalid status, receivedAt
and releasedAt when complete. Every duplicate Request appends another delivery;
UUID deduplication cannot hide duplicate gateway dispatch. A Java lock serializes
Item/JDBC updates. Exact committed integer counter and journal readback are
required. JDBC DecimalType serializes integers as e.g. `1.0`; numeric comparisons
accept only exact nonnegative safe integers, not fractional/string coercions.

The journal has a 128-row/65536-character bound. Exhaustion preserves existing
rows, increments the separate delivery counter and faults rather than evicting
history. Counter/journal gaps or mismatched restored state fault. This is a bounded
acceptance fixture, not sustained production retention or full-host rollback
protection. A fault invalidates the run's command-count evidence; do not reset it
or infer successful coverage from an incomplete journal.

## Repeatable preparation and checks

```sh
# Each preparation refuses any existing generation; never reuse by clearing state.
python3 deploy/scripts/prepare-held-canary.py --provisioning-env /protected/env
python3 deploy/scripts/prepare-held-canary.py --provisioning-env /protected/env --apply
python3 deploy/scripts/prepare-held-canary.py --provisioning-env /protected/env --inspect
python3 deploy/scripts/check-held-canary.py --provisioning-env /protected/env
# One local request, six-second hold, release, and release replay:
python3 deploy/scripts/check-held-canary.py --provisioning-env /protected/env \
  --apply --evidence /protected/new-exclusive-evidence.json
# Read-only current evidence; never exposes a credential:
python3 deploy/scripts/control-held-canary.py --provisioning-env /protected/env
# Explicit continuation of an existing held UUID, without another Request:
python3 deploy/scripts/control-held-canary.py --provisioning-env /protected/env \
  --release UUID
```

The check writes its UUID and prepared intent to an exclusive mode 0600 evidence
file, fsyncs it and its parent directory before sending the single Request. That
intent file is never rewritten. Later stages are published to a mode 0600
`<evidence>.progress` sibling using a same-directory temporary file, file fsync,
atomic replacement and directory fsync. Both names must be unused at preparation.
A failed stage write preserves the intent and the previous complete snapshot;
a failure after replacement can leave the new complete snapshot. An abrupt death
may leave an ignored temporary file. Preserve all evidence on failure and recover
only the original UUID; neither a prepared stage nor an incomplete progress file
proves that dispatch did not happen. Failure stops; no fresh retry is generated. Provision
and control inspect exact source/rule bindings, Item metadata, consumers and
channel links. They do not alter remote-enable, clear faults, reset stores or
send physical commands. Existing fixtures are refused, including failed ones.

Nine Node model tests and seven helper tests pass, including six injected stage-write
failures, pre-dispatch directory-sync failure and existing-evidence preservation. The real second-generation
fixture also passed: counter 0→1, matching UUID held beyond the shipped five-second
acknowledgement timeout, exact release then replay, counter still 1. Hold ON,
RemoteEnabled OFF, Fault OFF. No OpenHAB restart or cross-host test is claimed.
Source-pinned sanitized evidence is in
[home-held-canary-20260913.json](../testing/evidence/home-held-canary-20260913.json).

The first generation exposed the actual JDBC `0.0` versus Item `0` mismatch and
faulted before recording a held row. Its one attempted request is preserved in
`/tmp/lg-held-canary-first-check.json`; its count is not valid delivery evidence.
That generation's Items/rules and Fault ON remain untouched. A regression failed
before the numeric correction and passed after. The corrected second generation
uses distinct Item/rule/cache names; no old store or fault was cleared to fabricate
success.

## Cross-host sequencing and rollback

Use the observed nonzero baseline; do not reset Count 1 to obtain a fresh-looking
fixture. Preserve prior rows and verify no held request before the approved run.
VPS seeds only its separate synthetic provider/daemon store. The first actual
VPS-to-HOME request remains held while duplicate/concurrency, lost observation,
restart and paired-store checks execute. HOME verifies the exact UUID and releases
it; a second request completes under the agreed hold/release schedule. Require
exactly two additional delivery rows, two confirmations and 340 synthetic credits
remaining. Private coordination should carry the current UUID and evidence; it
must not carry the HOME token. Bounded failures invalidate the run, never justify
a fresh physical request or clearing a prior pending record.

A separate gateway service/config/store and reviewed authenticated path still
need preparation and acceptance after the allowlist ACK. This fixture does not
permit network exposure. Rollback closes that future gateway exposure first,
returns its remote switch OFF and preserves gateway/fixture histories; disable
only these fixture rules if needed. Never delete evidence, restore an older
physical/financial store or affect the original echo canary/household services.
