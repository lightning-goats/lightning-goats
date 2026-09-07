# Phase 1 Architecture — Strike-backed Lightning Goats

## Scope

Phase 1 replaces LNbits/Core Lightning for the goat-feeder payment path while preserving the existing durable feeder, Nostr, and overlay behavior.

CyberHerd business logic is explicitly deferred to Phase 2.

## Production topology

```text
Internet
   |
   v
new Vultr VPS
+--------------------------------------------------+
| nginx / TLS                                      |
| static lightning-goats.com                      |
|                                                  |
| lightning-goatsd                                 |
|   +-- native Lightning Address / LNURL-pay       |
|   +-- Strike receive adapter                     |
|   +-- SQLite durable ledger/event log            |
|   +-- feeder worker                              |
|   +-- template renderer                          |
|   +-- Nostr durable outbox                       |
|   +-- overlay WebSocket/status API               |
|                                                  |
| nak / NIP-46 client as required                  |
+---------------------------+----------------------+
                            |
                         WireGuard
                            |
                            v
                      trusted home network
                            |
                            +-- OpenHAB feeder rule
                            +-- FeederOverride item
                            +-- optional weather/status sources
```

External dependencies from the VPS:

- Strike API over HTTPS;
- Nostr relays / NIP-46 path;
- WireGuard-limited OpenHAB endpoints.

The Phase 1 VPS does **not** run:

- LNbits;
- Core Lightning;
- CLNRest;
- `clnaddress`;
- PostgreSQL for LNbits;
- a spend-capable wallet service.

## Lightning payment flow

### 1. Lightning Address discovery

A wallet resolves:

```text
herd@lightning-goats.com
```

through:

```text
GET /.well-known/lnurlp/herd
```

`lightning-goatsd` returns LNURL-pay metadata, callback URL, and configured min/max amounts.

### 2. Callback / invoice creation

The wallet calls the callback with an amount in millisatoshis.

`lightning-goatsd`:

1. validates the configured address and amount;
2. builds the exact LNURL metadata string;
3. hashes that metadata for the BOLT11 `descriptionHash`;
4. creates a BTC-denominated Strike receive request;
5. returns the BOLT11 invoice to the payer.

### 3. Settlement notification

Strike sends the configured webhook notification.

The webhook handler:

1. reads the request body without mutating state;
2. verifies the Strike webhook signature/HMAC;
3. extracts the referenced Strike entity ID;
4. fetches the authoritative receive/request state from Strike;
5. validates that it is completed and matches a receive request issued/accepted by this service;
6. converts it to the backend-neutral settlement domain object;
7. commits exactly once.

The webhook body itself is never authoritative financial state.

## Backend-neutral settlement contract

Phase 1 should converge on a model equivalent to:

```text
SettledPayment
- source              (Strike in Phase 1)
- source_id            unique provider settlement/receive identifier
- payment_hash         unique Lightning payment hash when available
- lightning_address    configured address/user
- amount_msat
- settled_at
- optional context     reserved for future identity/CyberHerd metadata
```

Idempotency must be enforced durably at the provider source ID and payment-hash boundaries where available.

No `pay_index` or CLN-specific ordering assumption belongs in this model.

## Durable financial/event transaction

A qualifying settlement performs one atomic transaction:

```text
verified Strike settlement
        |
        v
settlement record
        +
HERD_RECEIPT ledger entry
        +
payment_received durable event
        |
        commit
```

Only after the transaction commits may downstream presentation or feeder processing observe it.

Presentation failure must not undo or duplicate the settlement.

## Feeder flow

The existing feeder-accounting invariant remains canonical:

```text
feed_credit_sats / threshold_sats = feeds_due
feed_credit_sats % threshold_sats = remainder
```

Each physical feeding:

1. confirms no unresolved previous feed attempt;
2. confirms enough durable feed credit exists;
3. checks `FeederOverride` and fails safe if unavailable;
4. commits a feed intent;
5. invokes the configured OpenHAB rule;
6. if the result is ambiguous, marks the attempt `unknown` and blocks automatic retries;
7. if confirmed, debits exactly one threshold and commits `feeder_confirmed`.

Multiple earned thresholds are drained serially with the configured inter-feed delay.

## Messaging/event flow

Durable events are the source of presentation truth.

### Public event kinds

- `payment_received` -> `sats_received` template -> Nostr + overlay
- `feeder_confirmed` -> `feeder_trigger` template -> Nostr + overlay

### Overlay-only event kinds

- `interface_info`
- `weather_status`
- other explicitly classified informational presentation events

Audience is determined by the server-side event/message type. Do not trust an arbitrary payload field to decide whether something may publish to Nostr.

## Template renderer

Phase 1 ports only:

- payment-received fun goat-fact templates;
- feeder-trigger fun goat-fact templates;
- informational/interface/weather templates needed by the overlay.

Store templates as data (TOML/JSON/etc.) rather than large Rust source constants.

Rendering requirements:

- randomized selection;
- deterministic selection available in tests;
- only simple named placeholders;
- missing/malformed template handling fails safely;
- separate Nostr and overlay values are allowed (for example Nostr goat identity vs friendly overlay goat name/image).

## Nostr publication

Retain the existing architecture:

```text
durable event
  -> render
  -> NIP-46/nak sign
  -> persist exact signed event in outbox
  -> publish
  -> retry exact signed event on relay failure
```

Do not regenerate a new signed Nostr event for a publication retry.

The application must not store the Nostr private key directly.

## Overlay

Retain one read-only WebSocket event stream and a read-only status/snapshot API.

The overlay must be able to:

- obtain a durable snapshot on connect;
- consume ordered events with sequence numbers;
- detect gaps and reconnect/resnapshot;
- display payment and feeder messages;
- display overlay-only informational messages;
- never infer a physical feeding solely from a progress bar reaching a threshold.

Only committed `feeder_confirmed` events indicate physical feeder completion.

## Credentials

`lightning-goatsd` receives only:

- receive/read-only Strike API credential;
- Strike webhook verification secret;
- narrowly scoped OpenHAB credential;
- NIP-46 client credential/config as required.

It must not receive:

- Strike spend/withdraw authority;
- CLN rune/HSM material;
- LNbits wallet/admin keys;
- WireGuard private keys through application configuration;
- Nostr private signing key.

## Process boundaries

Production `lightning-goatsd` runs as a system-level systemd service under an unprivileged `lightning-goats` account.

nginx and WireGuard remain system services.

The Codex/deployment account is a separate identity and must not be the runtime account.

## Phase 2 compatibility

The payment, feeder, event, messaging, and overlay interfaces must not require CyberHerd state.

Future CyberHerd functionality may be implemented either:

- as a separate service consuming/producing durable Lightning Goats events; or
- as an internal module in the same codebase.

Phase 1 must preserve both choices.
