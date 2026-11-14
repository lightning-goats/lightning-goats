# Phase 1 messaging contract

This document defines the standalone `lightning-goatsd` presentation behavior for Phase 1.

## Source of truth

Financial and feeder state is committed first to the durable SQLite event log. Presentation is downstream and must never roll back or duplicate payment/feed state.

The shared `MessageRenderer` consumes durable events and produces two independent presentation targets:

- **Nostr**: public kind-1 text for payment and confirmed-feeder events only.
- **Overlay**: websocket messages for payment, feeder, interface-info, and weather events.

## Phase 1 event mapping

| Durable event | Template category | Nostr | Overlay type |
| --- | --- | --- | --- |
| `payment_received` | `sats_received` | yes | `sats_received` |
| `operator_credit_added` | `sats_received` | yes | `sats_received` |
| `feeder_confirmed` | `feeder_trigger` | yes | `feeder_trigger` |
| `interface_info` | `interface_info` | no | `interface_info` |
| `weather_status` | preformatted weather message | no | `weather_status` |

No other event type is publicly published by the Phase 1 renderer. Payment and
operator-credit announcements require at least 10 sats; smaller amounts still
credit the ledger normally. Operator credits use normal wording, as requested.

When a payment funds a new complete feeding, suppress its separate receipt text
on both Nostr and the overlay if its frozen feeding snapshot says ready and has
no unresolved feeding. A later `feeder_confirmed` event supplies the feeding
announcement. The payment still updates the overlay amount, QR and progress;
it never produces a feeding animation or success claim itself. A payment merely
adding to the next partial feeding retains its receipt text. This applies equally
to provider payments and operator test credits; the 10-sat message minimum remains.

Paused, unavailable, unresolved or missing feeding status keeps the normal
receipt/queue message. Selection uses the threshold frozen with the source event,
so subsequent configuration/status changes cannot change suppression on replay.
Readiness is advisory: a later refusal or pause can leave a suppressed payment
without an announcement until a feeding is confirmed; paid credit is preserved.
Already signed/published outbox entries are immutable and are not rewritten.

## Household signing and publication relays

`nostr.relays` lists public announcement destinations. Optional
`nostr.bunker_relays` selects the NIP-46 transport independently; when omitted,
existing configurations keep using `nostr.relays` for both operations. An
explicit empty or insecure bunker-relay list is rejected. Only signing receives
the client credential. Publication still sends the exact persisted signed event
to every configured announcement relay without contacting the bunker again.

The household connection uses Primal for NIP-46 signing. The operator-selected
announcement destinations are Primal, `wss://nostr.chaima.info/`,
`wss://relay.damus.io/`, and `wss://relay.artx.market/`. Adding a publication
destination does not add it to the household signer's connection or client
allowlist. See [household deployment](../deployment/household-nostr-bunker.md)
for actual verification and public-activation state.

## Deterministic selection

Template and goat selection is derived from SHA-256 over the durable event sequence/type plus a purpose namespace. It is intentionally not runtime-random.

This preserves variety between events while ensuring the same durable event renders identically after restart/retry. It complements the existing Nostr outbox invariant that relay retries publish the exact previously signed event rather than re-signing.

For payments to an individual goat Lightning Address, that goat is used for presentation. Payments to `herd` and feeder events select a goat deterministically.

Goat names are optional in the text. The selected original variant determines
whether `{goat_name}` appears; do not append a name to generic variants. The
operator-provided pool has 80 named/70 generic payment variants and 13 named/87
generic feeding variants. This preserves the legacy variety without changing a
message's wording on retry. The overlay can still show the associated goat image.

## Separate Nostr and overlay values

When a template uses `{goat_name}`:

- Nostr rendering substitutes the goat's `nostr:nprofile...` mention.
- Overlay rendering substitutes the human-readable goat name and adds `goats: [{name, imageUrl}]`.

The overlay image convention remains `images/<lowercase-goat>.png`.

## Safe substitution

Templates are data (`templates/cyberherd-legacy.json`), not executable code. Only
simple `{identifier}` placeholders are accepted. The import normalizes the sole
legacy `{new_amount:g}` modifier to `{new_amount}`; numeric strings are supplied
by the renderer. Attribute/index access such as `{x.__class__}` or `{x[0]}` is
rejected. Missing required fields fail rendering without changing the durable
source event or financial/feeder state.

## Initial template corpus

The 2026-11-09 operator export restores the original wording: 150 payment, 100
confirmed-feeding and 39 interface-information variants. The full imported file
contains 653 variants in 50 categories for the CyberHerd port. Loading those
additional categories does not grant them a Phase 1 publication audience. The
former `templates/phase1.toml` remains a historical adapted corpus, and is no
longer the embedded renderer source. Provenance and the incomplete CyberHerd
implementation are tracked in the [current plan](../planning/cyberherd-implementation-plan.md).

This describes the feature source; deployment/acceptance records determine which
binary and template corpus are actually running.

## Weather

`weather_status` remains overlay-only. Issue #21 owns weather polling/format construction compatibility; when it appends a durable `weather_status` event, the payload must contain a bounded non-empty `message` string and may carry structured `data` alongside it.
