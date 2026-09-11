# Phase 1 Architecture — Strike-backed Lightning Goats

## Scope

Phase 1 replaces LNbits/Core Lightning for the goat-feeder payment path while preserving the existing durable feeder, Nostr, overlay, and weather-information behavior.

CyberHerd business logic is explicitly deferred to Phase 2.

Canonical companion documents:

- `lightning-address-registry.md`
- `weather-overlay.md`
- `../security/openhab-feeder-gateway.md`
- `../security/phase1-threat-model.md`
- `../security/phase1-hardening-checklist.md`
- `../deployment/wireguard-topology.md`

## Existing WireGuard network

Phase 1 reuses the established network:

```text
10.8.0.0/24
```

Known nodes:

```text
10.8.0.1   current/old VPS WireGuard hub
10.8.0.6   in-house OpenHAB + weather host
```

During staging the new VPS uses a new keypair and an inventoried unused temporary `10.8.0.x` address. It never claims `10.8.0.1` while the old hub is active.

At preferred production cutover, after the old hub is stopped, the new VPS assumes `10.8.0.1/24` and existing clients update the hub public key/public Internet endpoint while retaining their own keys/addresses.

## Production topology

```text
Internet
   |
   v
new Vultr VPS / WireGuard hub 10.8.0.1
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
|   +-- weather/info scheduler                     |
|   +-- template renderer                          |
|   +-- Nostr durable outbox                       |
|   +-- overlay WebSocket/status API               |
|                                                  |
| nak / NIP-46 client as required                  |
+---------------------------+----------------------+
                            |
                  WireGuard 10.8.0.0/24
                            |
                            v
              10.8.0.6 in-house integration gateway
              +-----------------------------------+
              | narrow feeder/status/weather API  |
              | dedicated OpenHAB USER token      |
              | NO generic proxy                  |
              +----------------+------------------+
                               |
                  +------------+-------------+
                  |                          |
                  v                          v
              OpenHAB                local weather receiver
                  |                  127.0.0.1:5000
                  |                  GET /get_received_data
                  v
           physical feeder
```

External dependencies from the VPS:

- Strike API over HTTPS;
- Nostr relays / NIP-46 path;
- one narrow integration-gateway API over WireGuard.

The Phase 1 VPS does **not** run or hold:

- LNbits;
- Core Lightning;
- CLNRest;
- `clnaddress`;
- PostgreSQL for LNbits;
- a spend-capable wallet service;
- an OpenHAB API token;
- direct access to the legacy weather service at `10.8.0.6:5000`;
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

`lightning-goatsd` validates/canonicalizes the user, looks it up in the configured registry, and returns LNURL-pay metadata, callback URL, and configured min/max amounts.

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

The webhook handler verifies the Strike signature/HMAC, fetches authoritative Strike state, validates completion against a request issued by this service, converts it to the backend-neutral settlement domain object, and commits exactly once.

The webhook body itself is never authoritative financial state.

## Backend-neutral settlement contract

Phase 1 should converge on a model equivalent to:

```text
SettledPayment
- source              Strike in Phase 1
- source_id            unique provider settlement/receive identifier
- payment_hash         unique Lightning payment hash when available
- address_user         configured paid Lightning Address user
- credit_pool          herd for all Phase 1 addresses
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
4. asks the in-house gateway to process that exact UUID;
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

## Weather flow

The existing receiver is on `10.8.0.6:5000` and exposes both read and mutating routes, so it is not exposed to the VPS.

Flow:

```text
legacy receiver
127.0.0.1:5000/get_received_data
        |
        v
10.8.0.6 integration gateway
GET /v1/weather
(normalize + validate + sanitize)
        |
        v
lightning-goatsd weather scheduler
        |
        v
weather_status durable informational event
        |
        v
overlay only
```

Initial configurable defaults match the existing LNbits Lightning Goats extension:

```text
interval_seconds       = 60
broadcast_probability  = 0.30
```

Weather failures must be presentation-only. Weather must never enter the Nostr outbox in Phase 1.

See `weather-overlay.md` and issue #21.

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

Phase 1 ports only payment-received fun goat-fact templates, feeder-trigger fun goat-fact templates, and informational/interface/weather presentation needed by the overlay.

Store templates as data (TOML/JSON/etc.) rather than large Rust source constants. Rendering must support deterministic testing, simple named placeholders only, safe failure, and separate Nostr/overlay values where presentation differs.

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

Do not regenerate a new signed Nostr event for a publication retry. The application must not store the Nostr private key directly.

## Overlay

Retain one read-only WebSocket event stream and a read-only status/snapshot API.

The overlay must obtain a durable snapshot on connect, consume ordered events, detect gaps/reconnect, display payment/feeder/informational/weather messages, and never infer physical feeding solely from progress reaching a threshold.

Only committed `feeder_confirmed` events indicate physical feeder completion.

## Credentials

### VPS / `lightning-goatsd`

Receives only:

- receive/read-only Strike API credential;
- Strike webhook verification secret;
- NIP-46 client credential/config as required;
- optional low-value integration-gateway client credential if implemented in addition to WireGuard.

It must not receive:

- Strike spend/withdraw authority;
- OpenHAB token;
- CLN rune/HSM material;
- LNbits wallet/admin keys;
- WireGuard private keys through application configuration;
- Nostr private signing key.

### In-house integration gateway

Receives only:

- dedicated OpenHAB USER API token;
- optional gateway server credential material.

The gateway must not possess Strike or Nostr authority.

## Network boundaries

The existing `10.8.0.0/24` network is reused. Trusted-side firewalling on `10.8.0.6` permits only the gateway host/port required by Phase 1 from the approved staging/production VPS source.

Direct VPS access to `10.8.0.6:5000`, generic OpenHAB REST/admin, Postgres, internal SSH, and unrelated LAN/WireGuard services is blocked.

If the VPS also becomes the general WireGuard hub, routed client traffic policy must remain distinct from traffic originated by local VPS processes.

## Process boundaries

Production `lightning-goatsd` runs as a system-level systemd service under an unprivileged `lightning-goats` account.

The in-house integration gateway runs as a separate unprivileged system service/identity.

nginx and WireGuard remain system services.

The Codex/deployment account is a separate identity and must not be the runtime account.

## Phase 2 compatibility

The payment, feeder, weather, event, messaging, overlay, address-registry, and integration-gateway interfaces must not require CyberHerd state.

Future CyberHerd functionality may be implemented either as a separate service consuming/producing durable Lightning Goats events or as an internal module in the same codebase.

Any future CyberHerd feeder action must use the same durable feeder authority/gateway; it must not gain direct OpenHAB credentials. Any future payout authority should remain separable from the receive-only Phase 1 daemon.


## Audit correction: durable receives, invoice and currency contracts

Webhook acknowledgement now records authenticated, bounded notification IDs in
SQLite before replying. Provider reconciliation runs independently; issued
requests are scanned periodically even when no notification arrives. Durable
fair scheduling, pagination, retry and quarantine converge on the same atomic
settlement/credit/event transaction. Repeated failures never imply settlement.

At issuance, parse and verify the actual BOLT11 checksum, recoverable signature,
Bitcoin mainnet network, exact requested millisatoshis, payment hash and exact
LNURL metadata description hash. Optional wrapper fields may be absent; the
signed invoice still must bind all required values. Any supplied wrapper amount
or hash must agree. The signed expiry must equal the requested lifetime, the
invoice must be unexpired, and its creation time may be at most 30 seconds ahead
of the verifier. Recovery re-verifies the signed immutable fields and exact
stored invoice but does not reject a completed receive merely because its
invoice expired while notifications were delayed. Mainnet is the current
explicit supported invoice network; a sandbox returning another network fails
closed and requires a reviewed configuration/contract change before acceptance.

A completed LIGHTNING receive must have the issued amount and payment hash; any
supplied credited amount must agree in BTC. A completed P2P receive is bound to
the locally issued request ID and an authoritative BTC target. It requires
`amountCredited` in BTC, and credits that amount rather than the invoice face
value or a locally calculated conversion. BTC-to-BTC received and credited
amounts must agree. No payment hash is fabricated for P2P. Preserve the original
received and credited currency amounts, any supplied conversion rate and the
provider completion timestamp in settlement context. Supplied conversion
currency labels must agree with received currency and BTC target.

Rounding policy: **none**. The ledger accepts positive whole satoshis. Zero,
negative, non-decimal, overflow, fractional-satoshi or contradictory credited
amounts fail closed and remain durable retry/quarantine work. A future fractional
credit policy requires a separately reviewed accounting change. Never round up,
substitute the requested amount, or invent a conversion rate.

Contract reference inspected 2026-09-10:
[Strike receive schema](https://docs.strike.me/api/get-receives-for-receive-request/).
The parser is the locked
[lightning-invoice crate](https://docs.rs/lightning-invoice/latest/lightning_invoice/).
These are source contracts and locally signed mock fixtures, not live account
observations. Actual P2P account behavior remains an acceptance gate requiring
separate approval for any real payment.
