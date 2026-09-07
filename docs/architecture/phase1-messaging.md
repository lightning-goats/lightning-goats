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
| `feeder_confirmed` | `feeder_trigger` | yes | `feeder_trigger` |
| `interface_info` | `interface_info` | no | `interface_info` |
| `weather_status` | preformatted weather message | no | `weather_status` |

No other event type is publicly published by the Phase 1 renderer.

## Deterministic selection

Template and goat selection is derived from SHA-256 over the durable event sequence/type plus a purpose namespace. It is intentionally not runtime-random.

This preserves variety between events while ensuring the same durable event renders identically after restart/retry. It complements the existing Nostr outbox invariant that relay retries publish the exact previously signed event rather than re-signing.

For payments to an individual goat Lightning Address, that goat is used for presentation. Payments to `herd` and feeder events select a goat deterministically.

## Separate Nostr and overlay values

When a template uses `{goat_name}`:

- Nostr rendering substitutes the goat's `nostr:nprofile...` mention.
- Overlay rendering substitutes the human-readable goat name and adds `goats: [{name, imageUrl}]`.

The overlay image convention remains `images/<lowercase-goat>.png`.

## Safe substitution

Templates are data (`templates/phase1.toml`), not Rust source. Only simple `{identifier}` placeholders are accepted. Attribute/index access such as `{x.__class__}` or `{x[0]}` is rejected. Missing required fields fail rendering without changing the durable source event or financial/feeder state.

## Initial template corpus

The initial Phase 1 catalog is adapted from the existing `lightning-goats/cyberherd_messaging` payment, feeder, and interface-info seed templates. Obsolete CyberHerd wording is adapted to Lightning Goats where appropriate. The renderer is data-driven so larger historical pools can be added without redesign.

## Weather

`weather_status` remains overlay-only. Issue #21 owns weather polling/format construction compatibility; when it appends a durable `weather_status` event, the payload must contain a bounded non-empty `message` string and may carry structured `data` alongside it.
