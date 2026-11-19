# Durable native BTC receipt provenance

The Strike receiver verifies settlement before calling `record_payment`. The
ledger checks the original source identity, payment hash and exact conversion
from millisatoshis to satoshis.

## Records and transaction boundary

- `credit_valuations` stores one identity valuation per native BTC settlement,
  including its provider, receive scope, goat and credit pool.
- `asset_receipts` links the verified amount and source identity to the
  original `settled_payments` row.
- `credit_allocations` records the receipt's credited sats.
- `credit_grants` links that provenance to its ledger entry and event.

The receiver writes the settlement, ledger entry, public event and provenance
in the same serialized SQLite transaction. On duplicate delivery it verifies
the stored provenance and returns the original duplicate outcome. An error in
any write rolls back the transaction. Public payment messages and the overlay
use the credited sats and omit private provider identifiers.

## Existing databases

Migration 0010 backfills provenance for historical `settled_payments` by
referencing their existing `HERD_RECEIPT` entries. It preserves event bytes
and sequences, feed UUIDs, signed outbox data and issued Strike requests.
Historical grants leave the event reference unset. A missing or inconsistent
historical receipt entry aborts the migration.

The settlement record does not attest to a BTC network or wallet account. Its
provenance uses the existing `legacy-unspecified` and
`legacy-settled-payment` identifiers. Preserve applied migrations byte for
byte because SQLx verifies their checksums when opening an existing ledger.
