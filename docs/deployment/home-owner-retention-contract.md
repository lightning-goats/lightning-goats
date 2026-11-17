# Owner retention and restore contract — proposed for coordination

**Design proposal, not implemented or accepted. Production HOLD.** This makes
HOME's issue #17 comment5655433594 concrete. It builds on owner candidate PR72
and source-accepted v2 adapter PR76 at `bdd5a534a91015cdfadfc7f2f4fdc6ca69a74865`.
It does not replace the deployed physical owner or authorize compaction.

## Existing boundary and required outcome

The owner currently retains at most 32 receipts and refuses the 33rd distinct
request. This is safe exhaustion, not sustained operation. The gateway retains
requests and refusal tombstones in SQLite; v2 recovery additionally requires an
exact committed owner/JDBC completion. An owner Item, an HTTP success, or a local
replay witness alone must never become evidence of completed feeding.

The target is sustained operation beyond 32 completions without forgetting a
UUID that can still arrive, evicting unresolved work, weakening completion
parsing, or allowing restored state to authorize another physical actuation.
There is no finite safe time-to-live for replay identity under the current API.
Storage exhaustion must stop new admission, never delete identity evidence.

## Proposed division of work

HOME requests coordination for `src/gateway/owner_witness.rs`, the admission
integration in `src/gateway/server.rs`, narrowly necessary store interfaces,
new focused tests, and the existing owner candidate/contract. Public request and
result API shapes remain unchanged. VPS owns the real-daemon acceptance harness
and `tests/gateway_admission.rs`; HOME will not edit that test file.

The gateway witness is the first slice only. It protects a gateway-store rollback
when witness history survives. It is not sufficient authority to remove the
owner's 32-entry limit. All physical-owner ingress, including legacy callers,
must receive an owner-enforced durable replay check before owner compaction can
be enabled. A gateway-only check cannot protect a direct legacy owner invocation.
The owner-side durable backend/atomic admission mechanism needs a separate
reviewed implementation claim; this proposal does not assume OpenHAB Item
updates or eventual JDBC persistence provide a uniqueness transaction.

## Gateway witness and admission invariants

Use an explicitly initialized persistent witness store with schema version,
installation identity, and unique canonical request UUID records. Never silently
create it during ordinary startup when a configured store is missing, empty,
corrupt or from another installation. Existing gateway stores need an explicit,
quiesced migration, not an implicit schema toggle.

1. Preserve the existing gateway SQLite transaction that serializes admission,
   durable refusals, interval/hour capacity and unresolved reservation. Only its
   `New` outcome can reach a dispatch path; `Pending` can never resume dispatch.
2. For that original `New` outcome, commit a unique witness reservation before
   any owner POST. A witness hit, I/O failure or uncertain commit blocks POST and
   retains the gateway reservation. Never turn a witness hit into confirmation.
3. A witness record with no corresponding gateway row indicates incomplete or
   rolled-back state. Return a fail-closed error/ambiguity under the existing
   protocol; do not create a dispatchable request or claim `not_dispatched` as
   proof that the earlier owner saw nothing.
4. Existing gateway rows preserve their typed result/refusal behavior. A migrated
   refusal remains permanently non-dispatchable. A confirmed row must have
   reconciled identity coverage before startup admission is enabled.
5. A successful owner POST followed by a crash needs no special retry: the
   gateway reservation already exists. Recovery is GET-only for the same UUID,
   using the reviewed immutable owner completion contract. Never resend to fill
   a missing receipt or witness entry.

These stores are not an atomic two-database transaction. Crash after gateway
reservation but before witness commit leaves unresolved work and no automatic
POST. Crash after witness commit but before POST has the same conservative
outcome. Availability loss is reconciled explicitly; it cannot be repaired by
issuing a fresh UUID for an ambiguous request.

## Startup, full-host restore and owner compaction

Two co-restored local databases cannot prove that no later actuation occurred.
A checksum, local timestamp or persistent enabled flag does not solve this.
Physical admission must start disabled after every gateway/owner restart and
restore until explicit local reconciliation. Any eventual enable operation must
bind the current gateway and owner process generations through a local-only
administrative channel; a restorable Item/flag or a VPS API call is insufficient.
The mechanism must close admission if either generation changes. Its exact
implementation and tests require coordination before coding shared paths.

Reconciliation compares preserved gateway requests/refusals, owner durable
receipts and witness history. Import their UUID union without downgrading or
synthesizing terminal results. Conflicting records, missing newer history or an
unresolved request prevent enablement. Full-host rollback requires independent
post-backup evidence or operator resolution; absent that evidence, stay disabled.
An operator acknowledgement alone is not a proof that an old UUID is fresh.

Only after every owner ingress is guarded by durable unique identity evidence
may a later compaction slice replace the bounded owner Item receipt view. Keep
unresolved records and every replay tombstone indefinitely; retain sufficient
immutable completion evidence for the gateway's GET recovery. The existing
bounded JDBC reader must be reviewed again if receipt representation changes.
No arbitrary limit increase, store clearing or success-parser relaxation counts
as implementing retention.

## Required falsification tests before acceptance

| Boundary | Counterexample and required observation |
| --- | --- |
| Beyond the cap | More than 32 completed distinct requests; oldest UUID repeated after restart yields zero additional owner commands. Repeat beyond 128; storage pressure stops safely. |
| Incomplete work | Capacity reached with unresolved work; no eviction and no fresh dispatch. |
| Reservation crash | Death before/after each store commit and before/after owner POST; original UUID remains blocked or GET-recoverable, never resent. |
| Partial restore | Old gateway/new witness and new gateway/old witness; inconsistent coverage disables admission. |
| Full restore | Both stores and owner state restored before a later known actuation; startup remains disabled, including replay of a saved enable operation. |
| Parallel callers | Two processes, distinct/same UUIDs, and legacy owner ingress cannot bypass the physical owner's serialization or unique identity check. |
| Receipt loss | Lost result notification and retained terminal receipt recover by GET only; missing/contradictory terminal evidence remains ambiguous. |
| Migration | Nonempty requests, refusals and owner receipts are preserved; unknown schema/identity/conflicts cause zero owner commands and leave originals unchanged. |
| Recovery | Interrupted migration/compaction preserves immutable originals and can resume or fail closed without resetting stores. |

All destructive/crash cases use disposable fixtures. Applying a physical-owner
correction, changing legacy callers or enabling physical admission remain
separate operator-approved steps. HOME requests VPS feedback on admission order,
restore/enable semantics, additional shared files and ownership before code work.
