# Explicit v2 adapter and committed receipt recovery

This source increment follows the VPS ACK in issue17 comment5654927867 and the
independent acceptance of the owner candidate in PR72. It does not activate that
candidate, replace the physical owner or change the deployed gateway configuration.

## Configuration

Legacy `protocol = "feeder_request_v1"` and `protocol = "uuid_canary"` remain
unchanged. V2 requires its own ledger binding in an explicit table:

```toml
[openhab]
url = "http://127.0.0.1:8080/"
request_item = "GoatFeeder_ManualRequest"
ack_item = "GoatFeeder_ManualResult"
protocol = { feeder_request_v2 = { ledger_item = "GoatFeeder_OwnerLedgerV2" } }
override_item = "FeederOverride"
remote_enabled_item = "LightningGoatsRemoteEnabled"
```

This is a future binding, not an installation command. The dedicated ledger must
be separate from command, result and safety Items. The gateway retains its HOME
USER credential; no token or new credential is sent to VPS. Existing gateway
HTTP paths and typed responses are unchanged. VPS-owned admission tests and
accounting/client code are untouched.

Commands add exactly `version: "feeder-request-v2"` alongside the original UUID
and fresh UTC `requestedAt`. V1 does not gain a version field; bare UUID command
and receipt parsing stays restricted to the existing harmless canary names.

## Recovery authority and bounds

A v2 result notification alone never confirms completion. The current dedicated
ledger locates the original UUID. Accepted state remains pending. A complete
entry must have the exact v2 versions/status/reason, a valid admission time and
a nonregressing completion time. Unknown fields, duplicate identities, invalid
versions, oversized state and entries beyond the candidate's 32-entry bound fail
closed. State is limited to 8192 bytes; v1/canary state limits remain 4096.

A current complete Item can precede the JDBC commit. The adapter therefore reads
explicit `serviceId=jdbc` history around that entry's completion timestamp:
10 seconds before through 10 seconds after, ascending rows, page 0 onwards,
8 rows per page, at most 8 pages. `itemState`, `boundary` and `displayState` are
false to exclude synthetic observations. HTTP bodies are bounded 256KiB per page;
existing no-proxy/no-redirect and five-second per-request HTTP limits apply.
The normal gateway acknowledgement deadline still wraps this recovery operation.
A recovery GET may use multiple bounded requests; it never issues a POST.

The reader requires matching Item identity/count, in-window ordered row times,
strict bounded ledger snapshots, and an exact matching committed complete entry.
A changed admission/completion, post-completion accepted state, eviction inside
this result window or exhausted pagination fails closed. A short final page is
required; an early complete row cannot bypass an unread full page. Missing commit
evidence remains ambiguous. The response's datapoints count is only the number
of returned rows, not total historical coverage.

**This is not an all-history scan or an anti-rollback proof.** Confirmation relies
on the reviewed v2 owner's immutable completion contract: it never downgrades or
evicts complete receipts. The window follows that owner's bounded timestamp-to-
persistence path; delayed/deleted evidence safely leaves recovery ambiguous.
V1 history is never reinterpreted as v2 completion. Sustained retention and any
future compaction/witness protocol require their own coordinated changes and
restore proof. The fixed entry cap stays in force. Full-host restore after newer
physical work must remain disabled pending reconciliation; this adapter does not
make restoration of an old gateway/owner pair safe.

## Verification and remaining gates

`cargo +1.88.0 test --locked --test openhab_v2` runs six focused regressions:
explicit configuration and command envelope; notification versus committed
completion and client recreation; strict pending/failure/denial parsing; bounded
pagination and contradictory history; versions/duplicate IDs/size/time limits;
and an actual gateway process timeout/restart/lost-notification recovery.
The process test observes exactly one original command after GET recovery and
repeated POST of the already confirmed UUID. All upstreams are isolated loopback
fixtures with synthetic credentials; no live owner is contacted by these tests.

The existing real unlinked owner/JDBC fixture evidence remains in
[home-owner-v2-candidate.md](home-owner-v2-candidate.md). The new fixed-target `inspect_owner_v2_fixture` example has now also read that
existing receipt through actual OpenHAB/JDBC using the dedicated canary USER,
returning `Complete`. It ran as a transient non-admin loopback-only system service
with encrypted credential delivery, a root-owned executable and read-only system
protection. No command method is called by the probe. The fixture counter and
ON-delivery count stayed1 and actuator remained OFF. The first launch from
noexec `/run` failed before execution; a standard executable directory resolved
that without changing mount or security policy. See
[reader evidence](../testing/evidence/home-owner-v2-reader-20260913.json).
Independent VPS review, retained-state restart
and serialization, sustained retention/full-host restore, migration of legacy
callers, the approved authenticated network path and physical-owner replacement
remain open. No result parser broadening or automatic command retry closes them.

Rollback of an inactive source candidate is a binary/config rollback only. Once
v2 work has been admitted, preserve gateway reservations and all owner receipts;
do not downgrade the protocol, clear stores or substitute old state as a routine
rollback. Stop new admission and reconcile under the reviewed operational plan.
