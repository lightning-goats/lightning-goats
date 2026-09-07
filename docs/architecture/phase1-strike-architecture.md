# Phase 1 Architecture — Strike-backed Lightning Goats

## Scope

Phase 1 replaces LNbits/Core Lightning for the goat-feeder payment path while preserving the existing durable feeder, Nostr, and overlay behavior.

CyberHerd business logic is explicitly deferred to Phase 2.

Canonical companion documents:

- `lightning-address-registry.md`
- `../security/openhab-feeder-gateway.md`
- `../security/phase1-threat-model.md`
- `../security/phase1-hardening-checklist.md`

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
|   +-- configured address registry                |
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
                  dedicated/narrow WireGuard path
                            |
                            v
              trusted in-house feeder gateway
              +-----------------------------------+
              | narrow feeder/status API          |
              | dedicated OpenHAB USER token      |
              | NO generic proxy                  |
              +----------------+------------------+
                               |
                            localhost
                               |
                               v
                           OpenHAB
                               |
                               +-- LightningGoatsFeederRequest
                               +-- LightningGoatsFeederAck
                               +-- LightningGoatsRemoteEnabled
                               +-- FeederOverride
                               +-- optional temperature/status
                               |
                               v
                         physical feeder
```

External dependencies from the VPS:

- Strike API over HTTPS;
- Nostr relays / NIP-46 path;
- narrow feeder-gateway API over the approved WireGuard path.

The Phase 1 VPS does **not** run or hold:

- LNbits;
- Core Lightning;
- CLNRest;
- `clnaddress`;
- PostgreSQL for LNbits;
- a spend-capable wallet service;
- an OpenHAB API token;
- generic OpenHAB/LAN access.

## Lightning Address registry

The application uses generic LNURL routes but an explicit configured user registry.

Required Phase 1 users:

```text
herd
dexter
rowan
cosmo
newton
nova
```

These correspond to:

```text
herd@lightning-goats.com
dexter@lightning-goats.com
rowan@lightning-goats.com
cosmo@lightning-goats.com
newton@lightning-goats.com
nova@lightning-goats.com
```

All six credit the same feeder pool (`herd`) while preserving `address_user` in durable settlement/event context.

Unknown users must fail before any Strike API call. Nginx path catchall is routing only; it is not wildcard Lightning Address authorization.

See `lightning-address-registry.md` for the full contract.

## Lightning payment flow

### 1. Lightning Address discovery

A wallet resolves a configured address, for example:

```text
dexter@lightning-goats.com
```

through:

```text
GET /.well-known/lnurlp/dexter
```

`lightning-goatsd`:

1. validates/canonicalizes the user;
2. looks it up in the configured registry;
3. returns LNURL-pay metadata, callback URL, and configured min/max amounts.

Unknown users return a safe error/404 and create no Strike request.

### 2. Callback / invoice creation

The wallet calls the callback with an amount in millisatoshis.

`lightning-goatsd`:

1. validates the configured address and amount;
2. rejects invalid/unknown requests before provider contact;
3. builds the exact LNURL metadata string;
4. hashes that metadata for the BOLT11 `descriptionHash`;
5. applies application-level provider-call backpressure/rate limits;
6. creates a BTC-denominated Strike receive request;
7. persists enough request context for later reconciliation;
8. returns the BOLT11 invoice to the payer.

Nginx also rate-limits invoice-creating callback routes.

### 3. Settlement notification

Strike sends the configured webhook notification.

The public edge restricts the webhook to the exact expected route/method/content type/body size.

The webhook handler:

1. reads the request body without mutating financial state;
2. verifies the Strike webhook signature/HMAC using constant-time comparison;
3. parses the supported event/entity reference defensively;
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
- address_user         configured paid Lightning Address user
- credit_pool          `herd` for all Phase 1 addresses
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
settlement record (including address_user)
        +
HERD_RECEIPT ledger entry for credit_pool=herd
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
3. creates a durable feed-attempt UUID/intention;
4. asks the in-house feeder gateway to process that exact UUID;
5. the gateway/OpenHAB rule enforces local safety gates and duplicate suppression;
6. `lightning-goatsd` waits for/queries the authoritative same-UUID acknowledgement;
7. if the result is ambiguous, marks the attempt `unknown` (or remains safely pending for same-UUID resolution) and never submits a fresh automatic actuation;
8. if confirmed, debits exactly one threshold and commits `feeder_confirmed`.

Local OpenHAB authority must enforce at least:

- `LightningGoatsRemoteEnabled`;
- `FeederOverride`;
- duplicate request UUID suppression;
- minimum physical-feed interval;
- configured absolute safety/feed cap.

Multiple earned thresholds are drained serially with the configured delay, but local OpenHAB safety rules remain authoritative even if the VPS is compromised.

See `../security/openhab-feeder-gateway.md`.

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

`address_user` may be retained in event context for future presentation, but HTTP requesters cannot choose template/event/Nostr authority.

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

### VPS / `lightning-goatsd`

Receives only:

- receive/read-only Strike API credential;
- Strike webhook verification secret;
- NIP-46 client credential/config as required;
- optional low-value feeder-gateway client credential if implemented in addition to WireGuard.

It must not receive:

- Strike spend/withdraw authority;
- OpenHAB token;
- CLN rune/HSM material;
- LNbits wallet/admin keys;
- WireGuard private keys through application configuration;
- Nostr private signing key.

### In-house feeder gateway

Receives only:

- dedicated OpenHAB USER API token;
- optional gateway server credential material.

The gateway must not possess Strike or Nostr authority.

## Network boundaries

The VPS-to-home application path should use a dedicated WireGuard interface/key/subnet where practical, or equivalent per-peer firewall isolation.

The trusted-side firewall permits only the feeder-gateway host/port required by Phase 1.

The VPS must not directly reach generic OpenHAB REST/admin endpoints, Postgres, internal SSH, or unrelated LAN/WireGuard services.

If the VPS also becomes a general WireGuard hub, routed client traffic policy must remain distinct from traffic originated by local VPS processes.

## Process boundaries

Production `lightning-goatsd` runs as a system-level systemd service under an unprivileged `lightning-goats` account.

The in-house feeder gateway runs as a separate unprivileged system service/identity.

nginx and WireGuard remain system services.

The Codex/deployment account is a separate identity and must not be the runtime account.

## Phase 2 compatibility

The payment, feeder, event, messaging, overlay, address-registry, and feeder-gateway interfaces must not require CyberHerd state.

Future CyberHerd functionality may be implemented either:

- as a separate service consuming/producing durable Lightning Goats events; or
- as an internal module in the same codebase.

Any future CyberHerd feeder action must use the same durable feeder authority/gateway; it must not gain direct OpenHAB credentials.

Any future payout authority should remain separable from the receive-only Phase 1 daemon.
