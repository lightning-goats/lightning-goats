# Durable asset-credit storage — first integration slice (#95)

This implements persistence underneath [the multi-asset plan](multi-asset-payments.md).
It is not MoneroPay ingress, an oracle, a wallet scanner or permission to activate
XMR. Existing Strike invoice/settlement APIs remain externally unchanged. No
physical-owner, gateway, network, dependency or workflow change is required here.

## Records and the transaction boundary

`0010_asset_credit.sql` adds five private tables:

- `credit_valuations`: immutable native BTC identity or XMR quote plus canonical
  network/provider/account/receive binding, goat, pool and pinned credit ceiling.
- `credit_allocations`: cumulative granted atomic amount and credited sats, with
  an irreversible hold marker when evidence becomes inconsistent.
- `asset_receipts`: immutable per-receive receipt identity, native amount and
  trusted first-seen time; a pending observation can advance to unlocked.
- `credit_grants`: one allocation per eligible receipt, including zero-sat dust,
  referencing its ledger entry and public event when a positive grant occurs.
- `credit_conflicts`: private conflicting observations, deduplicated on retry.

`credit_migration_check` is a migration-only assertion table dropped before commit.
No permanent coordination or wallet-balance store is added.

All receipt/credit writers acquire `BEGIN IMMEDIATE` before reading prior state.
A new eligible receipt, cumulative amount, credit delta, ledger entry and public
`payment_received` event commit together. Exceptions/cancellation roll back the
transaction. A competing writer waits for the SQLite write lock within the existing
busy timeout, or fails without partial credit. Reconciliation may retry the same
receipt after a database error. It must never change the receipt identity to retry.

Only receipt keys within a canonical receive scope are unique: the same transaction
can legitimately pay different subaddresses. Conversely, a receive binding cannot
be assigned to a new quote ID to obtain duplicate credit. Adapters MUST use stable
wallet/account and exact subaddress identifiers, never aliases supplied by a viewer.
Asset/network/provider/account/receive binding is checked on every observation.
Quote IDs, bindings and receipt keys are private, not public payment-status tokens.

Pending observations are stored without credit. Each receipt's first-seen time is
immutable; `[issued_at, expires_at)` controls quote eligibility. Later unlock honors
that original time. Late/early receipts are retained without credit but do not block
an otherwise valid timely receipt from unlocking. Their outcome is `Held`; the
allocation-level hold remains unset unless a separate inconsistency is detected.

Conflicting amount/first-seen data, unlocked-to-pending regression, double-spend
observations, corrupt saved allocations or credit/storage limits persist an intent
hold. Original receipts/grants are not erased or silently amended. Recovery does
not automatically release that hold. `hold_xmr_credit_intent` is also available to
the future reconciliation adapter for regressed/missing aggregate history. Holds
block **future grants for that intent**, not unrelated Strike payments; they do not
retract previously issued credit, reverse physical feeds, or automatically stop the
whole feeder. Compensation/release policy remains separately reviewed work.

## Important boundary still required in #97/#98

`record_xmr_receipt` is a trusted internal library call, not an HTTP handler. Its
`unlocked` and `double_spend_seen` fields are observations supplied by the adapter;
this library cannot independently authenticate them or establish chain finality.
The bridge/reconciler must verify current per-intent provider state, stable receipt
aggregation, full-history/aggregate consistency, synchronization, and trusted
persisted first-seen provenance before calling it. A webhook or client JSON must
never be deserialized directly into credit authorization. Missing history cannot
be detected from a single receipt: explicitly hold the intent when full readback
regresses or conflicts. Filtering a provider response is not proof of completeness.

The stored quote must already have passed issuance freshness/size policy. Restoring
it reconstructs its original rational and ratio, not a new market quote. The rate
numerator/denominator use decimal TEXT to preserve all u64 values without SQLite
REAL rounding. Operational ceilings are explicit caller policy, not storage maxima.

## BTC compatibility and migration

`record_payment` still enforces its original source/payment-hash identity and exact
millisatoshi-to-satoshi rules, and emits the same existing event shape. It now uses
serialized writing and appends BTC identity provenance inside the **same** existing
settlement/ledger/event transaction. A failure in provenance storage also rolls back
the original payment. Duplicate delivery validates that provenance and returns
`Duplicate` without another credit/event.

Migration backfills metadata for historical `settled_payments` by referencing their
already-existing `HERD_RECEIPT` entries. It does not insert historical ledger entries,
rewrite event payloads/sequences, reset pending feed UUIDs, change outbox bytes/cursors,
or discard issued Strike requests. Historical grant event pointers stay NULL rather
than guessing an old event match. Legacy ledger rows lacking `settled_payments`
remain intact and are not misrepresented as verified native receipts.

The legacy settlement DTO lacks a network/account attestation. Provenance explicitly
uses `legacy-unspecified` / `legacy-settled-payment`, rather than inventing mainnet or
a wallet identity. XMR always requires explicit network/account/receive binding.
A corrupt/missing/inconsistent historical receipt entry aborts the entire migration;
there is no partial backfill, silent rounding or credit repair.

Stop **all** old/new ledger writers before upgrading. Back up the quiesced database
using the existing backup procedure and verify a copy first. Do not run an older
already-started daemon concurrently with the new schema: it lacks provenance hooks.
Do not downgrade by restoring stale paid state after new money arrives. Financial
rollback/receipt reconciliation is distinct from replacing an executable. Applying
this migration to live state is deployment work, not performed by publishing code.

## Public projection

The following describes the #103 implementation. The operator subsequently approved
native XMR amount plus credited sats on Nostr and the overlay (BTC remains sats-only).
See [the current messaging policy](multi-asset-payments.md#overlay-messaging-and-privacy).
The additional public fields/renderer are tracked in #99; do not mistake the existing
four-field projection for the completed dual-amount message feature.

New XMR credit events contain only `amount_sats` (new grant), `feed_credit_sats`
(absolute balance), `address_user` and `credit_pool`; the existing event streamer
adds type/sequence and the message renderer supplies goat presentation. No receipt,
quote, subaddress, transaction, network, account, native amount or capability leaks
through serialization. Private input types deliberately do not implement Serialize.
Zero-sat dust persists a grant/watermark but emits no misleading payment celebration.
Existing historical BTC event bytes are intentionally unchanged in this migration.

## Verification and follow-up

`tests/asset_credit_storage.rs` exercises actual file-backed SQLite, populated old
migrations, mixed-asset credit and two confirmed ledger debits, partial/dust/pending
receipts, immutable quotes, binding conflicts, scoped duplicate identities,
concurrent native/XMR writers, callback-style replay, persistent holds, integer
limits, injected failures at ledger/event/grant/allocation writes and public redaction.
The tests use synthetic receipts and direct confirmation of ledger attempts, **not**
MoneroPay, live XMR, the physical gateway, or proof of real feeding. #100 retains the
combined-service/selected-provider/restore acceptance work. Holds cannot be cleared
by these APIs; no automated refunds, trades, spending or compensation are implemented.
