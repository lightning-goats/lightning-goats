# Lightning Goats Phase 1 — LNbits Removal and Rust Migration Plan

**Repository:** `lightning-goats/lightning-goats`
**Related fork:** `santyr/clnaddress`
**Phase:** 1
**Primary objective:** Remove Lightning Goats from LNbits completely while preserving production service until a controlled cutover.

---

## 1. Goals

Phase 1 replaces the current LNbits-based Lightning Goats runtime with a standalone Rust service while preserving the existing production system until cutover.

The completed Phase 1 architecture must provide:

* no LNbits runtime dependency;
* Rust-based `lightning-goatsd`;
* LNURL-P and Lightning Address handling through `clnaddress`;
* support for multiple Lightning Addresses on the same Core Lightning node;
* strict identification of `herd@lightning-goats.com` payments;
* durable feed-credit accounting independent of wallet balance;
* automatic feeder control through OpenHAB;
* no public/manual feeder endpoint;
* a single WebSocket/API backend for the overlay;
* Nostr publishing through `nak`;
* Nostr project signing through a NIP-46 bunker;
* a restricted Core Lightning rune with no payment/spending authority;
* CyberHerd offline until Phase 2;
* no CyberHerd payouts or splits in Phase 1;
* coexistence with the existing LNbits system until cutover;
* explicit late-LNbits-invoice reconciliation after cutover.

---

## 2. Non-Goals

Phase 1 does **not** include:

* porting CyberHerd;
* CyberHerd member management;
* CyberHerd headbutt logic;
* CyberHerd payout calculation;
* outbound Lightning splits;
* a replacement for the operator's OpenHAB manual feeder controls;
* a public administrative UI;
* generalized management of unrelated Lightning Addresses beyond what `clnaddress` already provides.

Phase 2 will address CyberHerd and payout/split integration separately.

---

## 3. Target Architecture

```text
                              Internet
                                 |
                                 v
                               nginx
                                 |
                +----------------+----------------+
                |                                 |
                v                                 v
       /.well-known/lnurlp/*              /api/v1/status
       /lnurlp                            /ws/overlay
       /invoice/*                          |
                |                          |
                v                          v
          +------------+            +-------------------+
          | clnaddress |            | lightning-goatsd |
          | CLN plugin |            |       Rust        |
          +-----+------+            +---------+---------+
                |                             |
                | creates invoices            | restricted rune
                v                             v
                       +----------------------+
                       |    Core Lightning    |
                       |                      |
                       | money trust boundary |
                       +----------------------+
                                  ^
                                  |
                           paid invoices
                                  |
                                  v
                         lightning-goatsd
                                  |
                   +--------------+--------------+
                   |                             |
                   v                             v
                OpenHAB                        `nak`
                 feeder                         |
                                                v
                                         NIP-46 bunker
                                                |
                                                v
                                    Lightning Goats Nostr key
```

### Phase 1 authority boundaries

`lightning-goatsd` must:

* observe relevant incoming payments;
* maintain application accounting;
* trigger OpenHAB automatic feeding;
* provide overlay state/events;
* publish Nostr messages through a bunker.

`lightning-goatsd` must **not**:

* possess the unrestricted `lightning-rpc` socket;
* create arbitrary invoices;
* pay invoices;
* use `xpay`;
* withdraw funds;
* alter channels;
* manage plugins;
* hold the Lightning Goats project Nostr private key;
* expose a public write/admin API.

---

## 4. Core Accounting Model

Lightning Goats will no longer use an LNbits "herd wallet balance."

Instead, the application maintains a durable feed-credit ledger.

### Invariant

```text
feed_credit_sats =
    qualifying herd receipts
  + explicit migration credits
  - confirmed automatic feeds * feeder_threshold_sats
```

For a threshold of `1000 sats`:

```text
2340 sats received

feeds_due = floor(2340 / 1000) = 2
remainder = 2340 % 1000 = 340
```

Expected behavior:

```text
2340
  |
  +-- feed #1 confirmed --> 1340
  |
  +-- feed #2 confirmed --> 340
```

If `FeederOverride` is ON, credits continue to accumulate.

When the override is turned OFF, one feed is performed for **each complete threshold**.

Examples:

| Feed credit | Threshold | Feeds due | Remainder |
| ----------: | --------: | --------: | --------: |
|         750 |      1000 |         0 |       750 |
|        1000 |      1000 |         1 |         0 |
|        1250 |      1000 |         1 |       250 |
|        2000 |      1000 |         2 |         0 |
|        2340 |      1000 |         2 |       340 |
|       10550 |      1000 |        10 |       550 |

Feeds must always be serialized.

---

## 5. Physical Feeder State Model

A physical actuator cannot be made transactionally atomic with SQLite or Core Lightning.

The system must therefore prefer avoiding double-feeding over automatic retries.

### Required feed state flow

```text
COLLECTING
    |
    v
FEEDS_DUE
    |
    +---- FeederOverride ON ----> BLOCKED
    |                               |
    |                               +---- override OFF
    |
    v
FEED_INTENT_COMMITTED
    |
    v
OpenHAB request
    |
    +---- confirmed success ----> FEED_CONFIRMED
    |                                  |
    |                                  +---- debit exactly one threshold
    |
    +---- ambiguous/failure ------> FEED_UNKNOWN
```

### Rules

1. Never debit a threshold before the corresponding feed is confirmed.
2. `FEED_UNKNOWN` must halt automatic feeding.
3. `FEED_UNKNOWN` must never automatically retry.
4. Operator reconciliation may mark the attempt as:

   * physically fed; or
   * definitely not fed.
5. Operator manual feeding through OpenHAB does **not** debit Lightning Goats feed credit.
6. `FeederOverride` is checked before every queued feed.
7. New payments arriving while queued feeds are being drained simply increase `feed_credit_sats`.
8. A configurable inter-feed delay must separate successive automatic activations.

Example:

```text
2340 credit
feed #1 confirmed -> 1340
feed #2 ambiguous -> remain at 1340, enter FEED_UNKNOWN
```

---

## 6. Implementation Order

The recommended implementation sequence is:

1. Inventory and freeze the current production system.
2. Extend and harden `santyr/clnaddress`.
3. Create the Rust domain model and SQLite ledger.
4. Implement feed state machine and exhaustive tests.
5. Implement restricted CLNRest integration.
6. Implement OpenHAB integration.
7. Implement overlay API/WebSocket.
8. Implement Nostr messaging and transactional outbox.
9. Deploy `nak` NIP-46 bunker systemd service.
10. Deploy `lightning-goatsd` systemd service.
11. Add nginx canary routing.
12. Run real production-node canary/shadow validation.
13. Build legacy LNbits migration/reconciliation tooling.
14. Prepare production database and cutover runbook.
15. Cut over Lightning Addresses and Lightning Goats.
16. Reconcile late LNbits settlements through the grace period.
17. Remove LNbits after reconciliation completes.
18. Freeze Phase 1 and begin Phase 2 separately.

---

# Part I — Existing Production Inventory

## 7. Freeze and Inventory the Existing System

Before modifying production behavior, record the current environment.

Capture:

* Core Lightning version;
* active CLN plugins;
* CLNRest settings;
* LNbits version/configuration;
* `lightning_goats_extension` commit/version;
* CyberHerd commit/version;
* CyberHerd Messaging commit/version;
* all current Lightning Addresses;
* per-address descriptions;
* per-address minimum and maximum amounts;
* per-address Nostr/Zap behavior;
* `commentAllowed` behavior;
* invoice expiry;
* current `herd` LNbits wallet;
* current `herd` LNbits balance;
* feeder threshold;
* payment-message minimum;
* OpenHAB URL;
* OpenHAB feeder rule ID;
* `FeederOverride` item;
* current weather configuration;
* current Nostr relay set;
* current Nostr message templates;
* current nginx configuration;
* current overlay version and endpoints;
* all current relevant systemd units.

Suggested commands:

```bash
nginx -T
lightning-cli getinfo
lightning-cli plugin list
lightning-cli showrunes
ss -ltnp
systemctl cat <core-lightning-unit>
systemctl cat <lnbits-unit>
command -v nak
nak --version
sha256sum "$(command -v nak)"
```

Store the output as migration evidence.

Do **not** place secret values in the migration report.

The existing LNbits system remains authoritative throughout development and canary testing.

---

# Part II — `clnaddress` Fork Work

## 8. Add Address-Aware Invoice Labels

Current upstream behavior uses a random UUID as the invoice label.

Change the label format to:

```text
clnaddress:v1:<user>:<uuid>
```

Examples:

```text
clnaddress:v1:herd:550e8400-e29b-41d4-a716-446655440000
clnaddress:v1:sat:9f5624ef-...
clnaddress:v1:donate:1ea1ab5c-...
```

`lightning-goatsd` must credit only invoices whose label begins with:

```text
clnaddress:v1:herd:
```

All other incoming invoices on the CLN node must be ignored by Lightning Goats accounting.

This is a security boundary and should not depend on invoice descriptions, payer-provided metadata, or Zap contents.

---

## 9. Harden Lightning Address Usernames

The `clnaddress` fork should canonicalize usernames and reject unsafe names.

Recommended rules:

* lowercase canonical form;
* bounded length;
* reject `:`;
* reject path separators;
* reject control characters;
* reject ambiguous Unicode forms unless intentionally supported;
* use a deliberately narrow character set appropriate for Lightning Address usernames.

The label parser must never accept ambiguous or attacker-controlled structure.

---

## 10. Add Per-Address Configuration

Extend `clnaddress` user metadata with optional address-specific values:

```rust
min_sendable_msat: Option<u64>,
max_sendable_msat: Option<u64>,
comment_allowed: Option<u64>,
nostr_enabled: Option<bool>,
```

Global values remain defaults.

Existing `clnaddress-adduser` positional behavior should remain compatible.

Named/object configuration may expose the richer fields.

This allows different Lightning Addresses to coexist without sharing the same limits or features.

---

## 11. Preserve LNURL Comment Behavior

The fork should support LNURL callback comments when configured.

Add:

```rust
comment: Option<String>
```

to callback query parsing.

Rules:

* advertise `commentAllowed` only when enabled;
* enforce the configured maximum;
* reject oversized comments;
* do not log raw comments at normal log levels;
* do not use comments as payment identity.

Lightning Goats does not need to consume comments in Phase 1.

---

## 12. Improve Zap Receipt Key Handling

Add file-based secret loading:

```text
clnaddress-nostr-privkey-file=
```

Production rules:

* inline key and key file are mutually exclusive;
* production uses the key-file mechanism;
* the file must be readable only by the CLN service account;
* the key is a dedicated Zap-receipt identity;
* never reuse the main Lightning Goats project Nostr identity.

Future enhancement: NIP-46 support for `clnaddress` may be considered later but is not required for Phase 1.

---

## 13. `clnaddress` Test Requirements

Required tests include:

```text
herd invoice       -> clnaddress:v1:herd:<uuid>
other invoice      -> clnaddress:v1:<other>:<uuid>

unknown user       -> rejected
invalid user       -> rejected
colon in user      -> rejected
oversized comment  -> rejected

per-user min/max   -> enforced
NIP-57 validation  -> preserved
legacy adduser API -> preserved
```

Add regression tests proving that unrelated Lightning Addresses cannot be classified as herd payments.

---

# Part III — Rust Application

## 14. Repository and Binary

Repository:

```text
lightning-goats/lightning-goats
```

Primary binary:

```text
lightning-goatsd
```

Optional local operator tool:

```text
lightning-goatsctl
```

Suggested source layout:

```text
src/
  main.rs
  config.rs

  cln/
    mod.rs
    rest.rs
    invoice_watcher.rs
    classifier.rs

  ledger/
    mod.rs
    migrations.rs

  feeder/
    mod.rs
    openhab.rs
    worker.rs

  messaging/
    mod.rs
    templates.rs
    nostr.rs
    outbox.rs

  overlay/
    events.rs
    websocket.rs

  services/
    weather.rs
    bitcoin_price.rs

  control/
    socket.rs

tests/
```

Recommended Rust stack:

* Tokio;
* Axum;
* Reqwest using rustls;
* SQLx with SQLite;
* Serde;
* TOML;
* tracing;
* secrecy/zeroize where appropriate.

At crate level:

```rust
#![forbid(unsafe_code)]
```

Commit `Cargo.lock`.

---

# Part IV — Durable CLN Invoice Processing

## 15. Use `waitanyinvoice` with a Persistent Cursor

The application must consume paid invoices using a durable `lastpay_index`.

Persist the cursor in SQLite.

For every paid invoice:

```text
settlement
    |
    v
BEGIN TRANSACTION
    |
    +-- validate pay_index
    +-- validate payment_hash
    +-- classify invoice label
    +-- optionally credit herd ledger
    +-- record settlement
    +-- advance CLN pay_index cursor
    |
    v
COMMIT
```

Important:

* every paid invoice advances the cursor;
* only herd labels create feed credit;
* unrelated node invoices are recorded only as necessary for cursor continuity;
* never derive initial state by replaying the whole node automatically.

A new production database requires explicit initialization to the current CLN pay index.

---

## 16. Payment Idempotency

Enforce at least:

```text
payment_hash UNIQUE
pay_index UNIQUE
```

A duplicated notification, restart, repeated request, or replay must never increase `feed_credit_sats` twice.

---

## 17. Trusted Invoice Classification

Only labels matching:

```text
clnaddress:v1:herd:<valid-uuid>
```

are qualifying herd receipts.

Fail closed.

Do not credit:

* other `clnaddress` users;
* ordinary CLN invoices;
* legacy LNbits invoices through normal Phase 1 classification;
* malformed labels;
* payer-provided descriptions;
* Nostr events alone.

---

# Part V — SQLite

## 18. Suggested Tables

Use a small application-owned SQLite database.

Suggested tables:

```text
cln_cursor
settled_invoices
ledger_entries
feed_attempts
event_log
message_outbox
legacy_imports
```

Possible `ledger_entries` types:

```text
HERD_RECEIPT
LEGACY_OPENING_CREDIT
LEGACY_SETTLEMENT_IMPORT
FEED_DEBIT
OPERATOR_RECONCILIATION
```

No secret values belong in SQLite.

---

## 19. SQLite Durability Settings

Use:

```text
journal_mode=WAL
synchronous=FULL
foreign_keys=ON
busy_timeout=<reasonable value>
```

Favor correctness and durability over marginal write performance.

Only one automatic feeder worker may operate at once.

---

# Part VI — Feed Worker and OpenHAB

## 20. OpenHAB Responsibilities

`lightning-goatsd` may:

* read `FeederOverride`;
* invoke the predefined automatic feeder rule;
* read any required status/telemetry;
* fetch existing Bitcoin price data if still desired.

It must **not** expose a manual feeder HTTP endpoint.

Operator manual feeding remains entirely inside OpenHAB.

---

## 21. Feed Worker Rules

A feed may execute only when:

```text
feed_credit_sats >= feeder_threshold_sats
AND FeederOverride == OFF
AND no feed is currently pending/unknown
```

After each confirmed feed:

```text
feed_credit_sats -= feeder_threshold_sats
```

Then re-evaluate.

A configurable delay separates multiple earned feeds:

```toml
[feeder]
threshold_sats = 1000
inter_feed_delay_seconds = 30
```

The final delay should be chosen based on the physical feeder.

---

## 22. Override Behavior

While override is ON:

* payments continue accumulating;
* the overlay shows accumulated progress;
* no automatic feeder activation occurs.

When override turns OFF:

* execute one feed for every complete threshold;
* retain the remainder.

Example:

```text
2340 sats accumulated overnight
override OFF

feed
feed

remaining feed credit = 340 sats
```

If the operator re-enables override during backlog draining, stop before the next feed.

---

## 23. Local Reconciliation

If feed actuation becomes ambiguous, the daemon enters `FEED_UNKNOWN`.

A local-only operator tool may reconcile:

```bash
lightning-goatsctl reconcile-feed <feed-id> --fed
```

or:

```bash
lightning-goatsctl reconcile-feed <feed-id> --not-fed
```

This tool must **not** directly trigger the feeder.

It only resolves accounting ambiguity.

Prefer communication over a root/operator-accessible Unix-domain socket rather than a public network API.

---

# Part VII — CLNRest and Runes

## 24. Keep Existing CLNRest During Migration

Do not modify the current CLNRest configuration if LNbits still depends on it.

The new daemon connects to the existing local CLNRest instance.

CLNRest should remain bound only to localhost/internal interfaces.

Do not expose CLNRest through nginx.

---

## 25. Create a Dedicated Restricted Rune

The Phase 1 rune should authorize only what is needed for payment observation.

Target permission set:

```text
waitanyinvoice
listinvoices
```

`getinfo` should be omitted unless a concrete requirement appears.

Explicitly verify that the rune denies:

```text
invoice
pay
xpay
withdraw
close
plugin
setchannel
fundchannel
```

Use `checkrune` against the installed CLN version before deployment.

Store the final rune as a systemd encrypted credential.

`lightning-goatsd` must not have permission to read the unrestricted `lightning-rpc` socket.

---

# Part VIII — Nostr and NIP-46

## 26. Nostr Identity Separation

Use two different Nostr identities:

### Main Lightning Goats project identity

Used for:

* sats received messages;
* feeder messages;
* weather messages;
* interface/status messages.

Storage:

```text
NIP-46 bunker only
```

The actual project nsec must not exist in:

* `lightning-goatsd`;
* its SQLite database;
* its TOML file;
* its command-line arguments.

### `clnaddress` Zap receipt identity

Used only for NIP-57 receipts.

Use a separate dedicated key.

---

## 27. Generate a Dedicated NIP-46 Client Key

Generate a new key for `lightning-goatsd` to authenticate to the bunker.

Example:

```bash
nak key generate
```

Derive its public key.

The bunker authorizes the client pubkey.

The daemon stores only the client secret.

The client key is not the Lightning Goats project key.

---

## 28. NIP-46 Bunker systemd Unit

Create a dedicated service such as:

```text
lightning-goats-nostr-bunker.service
```

Recommended unit:

```ini
[Unit]
Description=Lightning Goats NIP-46 Nostr Bunker
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
DynamicUser=yes

RuntimeDirectory=lightning-goats-bunker
UMask=0077

LoadCredentialEncrypted=nostr-key:/etc/credstore.encrypted/lightning-goats-nostr.key

ExecStart=/usr/local/libexec/lightning-goats/run-nak-bunker

Restart=on-failure
RestartSec=5s

NoNewPrivileges=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectSystem=strict
ProtectHome=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
ProtectKernelLogs=yes
ProtectClock=yes
ProtectHostname=yes
ProtectProc=invisible
ProcSubset=pid
RestrictSUIDSGID=yes
RestrictRealtime=yes
LockPersonality=yes
MemoryDenyWriteExecute=yes
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6
CapabilityBoundingSet=
AmbientCapabilities=

[Install]
WantedBy=multi-user.target
```

Use the actual installed path from:

```bash
command -v nak
```

---

## 29. Bunker Wrapper Script

Example:

```sh
#!/bin/sh
set -eu

export HOME=/run/lightning-goats-bunker
export NOSTR_SECRET_KEY="$(cat "$CREDENTIALS_DIRECTORY/nostr-key")"

exec /usr/local/bin/nak \
  --config-path /run/lightning-goats-bunker \
  bunker \
  --authorized-keys "$LG_NOSTR_CLIENT_PUBKEY" \
  wss://relay-one.example \
  wss://relay-two.example
```

The project nsec must not appear in process arguments.

Do not use `nak bunker --persist` for the project key.

The systemd credential remains the key source of truth.

A same-host bunker protects the project key from compromise of the application process, but not from root-level host compromise. Moving the bunker to a separate host later should remain architecturally possible.

---

## 30. Nostr Publishing from `lightning-goatsd`

Use `tokio::process::Command`.

Never use:

```text
sh -c
```

The daemon supplies:

```text
NOSTR_SECRET_KEY=bunker://...
NOSTR_CLIENT_KEY=<dedicated client secret>
```

to the child process environment.

The bunker URL is configuration; the client secret is a systemd credential.

### Transactional Nostr outbox

For reliable publishing:

1. build the event;
2. sign through `nak` + bunker;
3. capture the signed event JSON;
4. store the exact signed event in `message_outbox`;
5. publish it;
6. mark success;
7. on retry, republish the exact same signed event.

Retries must not generate a new Nostr event ID.

---

# Part IX — Messaging

## 31. Phase 1 Message Categories

Port only Phase 1 categories:

```text
sats_received
feeder_triggered
interface_info
weather_status
processing_error
```

Do not port CyberHerd membership/headbutt messages until Phase 2.

Export the existing production message templates before cutover and convert them into application-owned template data.

---

## 32. Shadow-Mode Nostr Behavior

While the new daemon is in shadow mode:

```text
Nostr signing test       allowed
public Nostr publishing  disabled
```

Signing may be tested without public relay publication.

Shadow mode must never duplicate live LNbits messages.

---

# Part X — Overlay

## 33. Single Overlay WebSocket

The overlay should move from multiple legacy websocket sources to:

```text
wss://lightning-goats.com/ws/overlay
```

On connection, the daemon sends a complete snapshot.

Example:

```json
{
  "type": "snapshot",
  "seq": 41882,
  "feed_credit_sats": 340,
  "threshold_sats": 1000,
  "feeds_due": 0
}
```

Payment event:

```json
{
  "type": "payment_received",
  "seq": 41883,
  "amount_sats": 500,
  "feed_credit_sats": 840,
  "threshold_sats": 1000
}
```

Feed event:

```json
{
  "type": "feeder_confirmed",
  "seq": 41884,
  "feed_credit_sats": 340
}
```

Sequence numbers allow clients to detect gaps and refresh via a new snapshot.

The browser must never receive:

* a CLN rune;
* LNbits keys;
* CLN API credentials;
* OpenHAB credentials;
* NIP-46 client secrets.

---

## 34. Public API Surface

Keep the public HTTP surface deliberately small.

Target:

```text
GET /healthz
GET /api/v1/status
WS  /ws/overlay
```

Add only genuinely required read-only endpoints.

No public administrative write endpoints.

No public manual feeder endpoint.

---

# Part XI — `lightning-goatsd` systemd Deployment

## 35. Service Account

Use a dedicated account:

```text
User=lightning-goats
Group=lightning-goats
```

Do not add this account to the Core Lightning group if doing so grants access to `lightning-rpc`.

---

## 36. Suggested systemd Unit

```ini
[Unit]
Description=Lightning Goats
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=lightning-goats
Group=lightning-goats

ExecStart=/usr/local/bin/lightning-goatsd --config /etc/lightning-goats/config.toml

StateDirectory=lightning-goats
RuntimeDirectory=lightning-goats
ConfigurationDirectory=lightning-goats

UMask=0077

LoadCredentialEncrypted=cln-rune:/etc/credstore.encrypted/lightning-goats-cln-rune
LoadCredentialEncrypted=openhab-token:/etc/credstore.encrypted/lightning-goats-openhab
LoadCredentialEncrypted=nostr-client-key:/etc/credstore.encrypted/lightning-goats-nostr-client

Restart=on-failure
RestartSec=5s

NoNewPrivileges=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectSystem=strict
ProtectHome=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
ProtectKernelLogs=yes
ProtectProc=invisible
ProcSubset=pid
RestrictSUIDSGID=yes
RestrictRealtime=yes
LockPersonality=yes
MemoryDenyWriteExecute=yes
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6
CapabilityBoundingSet=
AmbientCapabilities=

[Install]
WantedBy=multi-user.target
```

Adjust restrictions only when a verified dependency requires it.

The daemon should listen only on:

```text
127.0.0.1:<port>
```

---

# Part XII — nginx

## 37. Preserve the Existing nginx Installation

Do not replace nginx.

Before changes:

```bash
nginx -T > /root/nginx-pre-lightning-goats-migration.conf
```

Always test before reload:

```bash
nginx -t
systemctl reload nginx
```

---

## 38. Internal Upstreams

Example:

```nginx
upstream clnaddress_backend {
    server 127.0.0.1:9797;
    keepalive 8;
}

upstream lightning_goats_backend {
    server 127.0.0.1:8787;
    keepalive 8;
}
```

Confirm actual unused ports first with:

```bash
ss -ltnp
```

---

## 39. Canary Lightning Address

Before switching production addresses, create:

```text
herd-canary@lightning-goats.com
```

in `clnaddress`.

Route only that exact address to `clnaddress`.

Example:

```nginx
location = /.well-known/lnurlp/herd-canary {
    proxy_pass http://clnaddress_backend;
    add_header Access-Control-Allow-Origin "*" always;
}
```

Also route the `clnaddress` invoice callback:

```nginx
location ^~ /invoice/ {
    proxy_pass http://clnaddress_backend;
    add_header Access-Control-Allow-Origin "*" always;
}
```

Production `herd@lightning-goats.com` and all existing Lightning Addresses remain on LNbits at this stage.

---

## 40. Overlay Routing During Shadow Mode

Expose a canary/staging WebSocket path if useful, for example:

```text
/ws/overlay-canary
```

Do not switch the production overlay until the daemon has passed shadow validation.

---

# Part XIII — Shadow and Canary Testing

## 41. Shadow Mode

The daemon must have an explicit deployment mode:

```text
shadow
active
```

In shadow mode:

```text
CLN observation          YES
SQLite accounting        YES
feed decisions           YES
OpenHAB reads            YES
OpenHAB feeder writes     NO
public Nostr publishing   NO
production overlay        NO
```

Shadow mode is not just a logging flag. It must hard-disable external side effects.

---

## 42. Use a Separate Canary Database

Canary testing must use a database that will never become the production ledger.

Example configuration:

```text
herd_user = "herd-canary"
mode = "shadow"
```

Archive the canary DB after testing.

Do not reuse it for production.

---

## 43. OpenHAB Test Rule

Create an OpenHAB test rule that increments a counter or toggles a harmless test item rather than operating the physical feeder.

Use this to validate backlog semantics.

Example test:

```text
payment = 2340 sats
threshold = 1000

expected:
test actuator executions = 2
remaining ledger         = 340
```

---

## 44. Required Canary Cases

Test at least:

* 1 sat;
* 999 sats;
* 1000 sats;
* 1250 sats;
* 2000 sats;
* 2340 sats;
* 10550 sats;
* multiple concurrent settlements;
* daemon restart;
* CLNRest interruption;
* duplicate/replayed settlement;
* payment to another Lightning Address;
* ordinary non-clnaddress CLN invoice;
* malformed labels;
* override ON;
* override OFF;
* override enabled during queued feeding;
* NIP-57 Zap;
* NIP-46 signing;
* WebSocket reconnect;
* SQLite restart/crash recovery;
* OpenHAB timeout;
* `FEED_UNKNOWN` reconciliation.

---

# Part XIV — Production Database Preparation

## 45. Create a Fresh Production Database

Do not promote the canary DB.

Production configuration:

```text
herd_user = "herd"
tracked_prefix = "clnaddress:v1:herd:"
```

Initialize its CLN cursor deliberately to the intended production boundary.

Do not automatically import historical CLN invoices.

---

# Part XV — Legacy LNbits Migration Tool

## 46. Why It Is Required

Changing nginx prevents new LNURL invoices from being created through LNbits, but old LNbits-created invoices may still be unpaid and valid.

Those invoices may settle after cutover.

They must not be lost.

---

## 47. Build a Temporary Legacy Reconciliation Tool

Create a migration-only tool capable of reading the legacy LNbits herd payment data.

It may connect to LNbits during the migration grace period.

The permanent `lightning-goatsd` must not depend on LNbits.

For each eligible post-cutover settlement, import:

```text
source = legacy_lnbits
payment_hash
amount_sats
settled_at
legacy_invoice_id/checking_id
```

The import must be idempotent by `payment_hash`.

Use a separate `legacy_imports` table or equivalent audit record.

---

## 48. Opening Credit

Immediately before cutover, record the current LNbits herd wallet balance.

Import it once as:

```text
LEGACY_OPENING_CREDIT
```

Example:

```text
LNbits herd balance at boundary = 620 sats

new ledger opening credit = +620 sats
```

Record:

* exact UTC timestamp;
* exact old wallet ID;
* observed balance;
* import transaction ID;
* operator/cutover record.

---

# Part XVI — Production Cutover

## 49. Pre-Cutover Conditions

Do not cut over until:

* `clnaddress` fork tests pass;
* `lightning-goatsd` tests pass;
* restricted rune is verified;
* canary address works;
* real CLN settlements are classified correctly;
* unrelated addresses are ignored;
* NIP-57 behavior is verified;
* NIP-46 signing works;
* overlay canary works;
* OpenHAB test rule proves feed semantics;
* restart behavior is verified;
* legacy migration tool is ready;
* rollback configuration is prepared.

---

## 50. Cutover Step A — Enable OpenHAB Override

Set:

```text
FeederOverride = ON
```

This freezes physical automatic feeding during accounting transfer.

Payments may continue arriving.

---

## 51. Cutover Step B — Record the Boundary

Record:

* UTC cutover timestamp;
* current LNbits herd balance;
* last known legacy herd settlement;
* current CLN pay index;
* all known unexpired legacy invoices;
* current production commit hashes;
* current nginx config checksum.

Import the current LNbits herd balance as the production ledger opening credit.

---

## 52. Cutover Step C — Disable Old Side Effects

Disable or stop the active behavior of:

* Lightning Goats LNbits extension;
* CyberHerd;
* CyberHerd Messaging jobs that publish or actuate.

Do **not** stop LNbits itself yet.

LNbits must remain available for late legacy-invoice reconciliation.

---

## 53. Cutover Step D — Switch Lightning Address Routing

Switch production:

```text
/.well-known/lnurlp/<user>
```

routes from LNbits to `clnaddress`.

All production users must already exist in `clnaddress`.

The public Lightning Addresses remain unchanged.

---

## 54. Cutover Step E — Activate `lightning-goatsd`

Set:

```text
mode = active
```

Keep `FeederOverride` ON initially.

Verify the daemon is receiving new `clnaddress:v1:herd:*` settlements and crediting them.

---

## 55. Cutover Step F — Verify a Real Herd Payment

Send a real small payment through:

```text
herd@lightning-goats.com
```

Verify:

```text
Lightning Address
        |
        v
clnaddress
        |
        v
clnaddress:v1:herd:<uuid>
        |
        v
Core Lightning settlement
        |
        v
lightning-goatsd
        |
        v
SQLite ledger
        |
        v
overlay event
```

No physical feed should occur while override remains ON.

---

## 56. Cutover Step G — Release Override

Set:

```text
FeederOverride = OFF
```

The daemon must drain all full thresholds sequentially.

Example:

```text
feed_credit_sats = 2340

feed
feed

remaining = 340
```

Confirm the actual physical behavior and ledger results.

---

# Part XVII — Late LNbits Invoice Grace Period

## 57. Keep LNbits Running Temporarily

After route cutover:

* new Lightning Address invoices come from `clnaddress`;
* old LNbits invoices may still settle;
* LNbits remains available only for migration reconciliation.

The legacy importer periodically checks for newly settled pre-cutover herd invoices.

---

## 58. Temporary Reconciliation Timer

A temporary systemd timer may run the migration reconciliation tool.

Each run:

1. fetches eligible legacy settlements;
2. filters to the old herd wallet;
3. filters to invoices created before the cutover boundary;
4. imports unseen payment hashes;
5. writes an audit record;
6. never reimports duplicates.

Do not make this timer part of the permanent Phase 1 architecture.

---

## 59. End of Grace Period

After the maximum legacy invoice expiry plus a safety margin:

1. run a final reconciliation;
2. verify no relevant unexpired legacy invoices remain;
3. verify all eligible late settlements were imported;
4. archive the reconciliation report;
5. disable/remove the temporary timer;
6. stop LNbits.

---

# Part XVIII — LNbits Removal

## 60. Remove LNbits from Runtime

Once reconciliation is complete:

* stop LNbits;
* disable the LNbits systemd unit;
* remove obsolete nginx routes;
* remove old LNbits WebSocket dependencies from the overlay;
* revoke obsolete LNbits keys;
* archive the LNbits database/configuration;
* retain a read-only audit copy for a defined retention period.

At this point:

```text
LNbits runtime dependency = ZERO
CyberHerd                 = OFFLINE
outbound herd splits      = NONE
```

Phase 1 is complete.

---

# Part XIX — Rollback

## 61. Pre-Payment Rollback

Before the first production `clnaddress:v1:herd:*` payment settles, rollback is simple:

1. keep or restore `FeederOverride = ON`;
2. return production Lightning Address nginx routes to LNbits;
3. re-enable old Lightning Goats/CyberHerd behavior;
4. disable new daemon side effects;
5. verify the old overlay path;
6. release override when safe.

---

## 62. Point of No Blind Rollback

The first confirmed production:

```text
clnaddress:v1:herd:*
```

settlement is the **point of no blind rollback**.

After that event, CLN has received funds that do not exist inside the LNbits virtual herd-wallet accounting model.

A rollback after this point requires explicit reconciliation.

Never assume the new application ledger and old LNbits wallet can simply replace each other.

---

# Part XX — Security Hardening

## 63. General Security Requirements

The production design should ensure that compromise of `lightning-goatsd` does **not** automatically grant:

* spend authority over the CLN node;
* access to `lightning-rpc`;
* access to the Lightning Goats project nsec;
* plugin administration;
* CyberHerd payout authority;
* public manual feeder activation.

---

## 64. Secret Handling

Use systemd credentials for:

```text
CLN rune
OpenHAB credential
NIP-46 client key
Lightning Goats bunker project key
clnaddress Zap-receipt key
```

Avoid ordinary environment files for long-lived secrets where systemd credentials are available.

Never log secret values.

Never place secrets in:

* Git;
* SQLite;
* command-line arguments;
* nginx configuration;
* browser code;
* API responses.

---

## 65. Process Hardening

Both new systemd units should use as much of the following as practical:

```text
NoNewPrivileges=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectSystem=strict
ProtectHome=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
ProtectKernelLogs=yes
ProtectProc=invisible
ProcSubset=pid
RestrictSUIDSGID=yes
RestrictRealtime=yes
LockPersonality=yes
MemoryDenyWriteExecute=yes
CapabilityBoundingSet=
AmbientCapabilities=
```

Maintain network access only where required.

---

## 66. Public Network Boundary

Only nginx should accept public Internet connections.

Keep:

```text
clnaddress       -> loopback only
lightning-goatsd -> loopback only
CLNRest          -> loopback only
```

Expose only required nginx locations.

---

## 67. Logging

Use structured logs.

Never log:

* CLN rune;
* Nostr private/client keys;
* OpenHAB credentials;
* complete raw secret-bearing configuration;
* unnecessary Zap request contents;
* raw LNURL comments at normal levels.

Include stable event IDs, pay indexes, payment hashes where operationally appropriate, feed attempt IDs, and state transitions.

---

# Part XXI — CI and Verification

## 68. Rust CI

Required:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
cargo audit
cargo deny check
```

Recommended:

* dependency lockfile review;
* reproducible release process;
* binary checksum at deployment;
* SBOM generation;
* CodeQL or equivalent static analysis where useful.

---

## 69. Security Tests

Required tests should prove:

### Payment safety

```text
duplicate payment never increases credit twice
non-herd payment never increases feed credit
malformed label never increases feed credit
cursor restart never replays credit
```

### Feeder safety

```text
no ledger debit without FEED_CONFIRMED
FEED_UNKNOWN never automatically retries
override ON causes zero automatic activations
2340 @ 1000 results in exactly 2 confirmed feeds + 340
10550 @ 1000 results in exactly 10 confirmed feeds + 550
```

### Authority safety

```text
CLN rune cannot pay
CLN rune cannot withdraw
CLN rune cannot create arbitrary invoice
CLN rune cannot modify channels
public API cannot trigger feeder
public API cannot change configuration
```

### Nostr safety

```text
project nsec is absent from daemon config/db
failed publish retries same signed event ID
NIP-46 client key cannot be returned via API/logs
```

---

## 70. Property-Based Feed Invariant

For arbitrary sequences of:

* qualifying payments;
* non-qualifying payments;
* duplicate settlements;
* restarts;
* override changes;
* successful feeds;
* failed feeds;
* ambiguous feeds;

the system must maintain:

```text
total debited feed thresholds
<=
confirmed physical automatic feeds * threshold
```

and:

```text
feed_credit_sats
=
qualifying credits
- confirmed feed debits
```

The application must never create more confirmed feed debits than confirmed physical automatic feeds.

---

# Part XXII — Operational Acceptance Criteria

## 71. Phase 1 Definition of Done

Phase 1 is complete only when all of the following are true:

### Payment ingress

* `clnaddress` serves all required Lightning Addresses.
* Address-specific labels are present.
* `herd@lightning-goats.com` is uniquely identifiable.
* NIP-57 support works where required.
* comments/limits required by existing addresses are preserved.

### Lightning Goats daemon

* `lightning-goatsd` is Rust.
* no `unsafe` application code is allowed.
* payment processing is restart-safe.
* payment processing is idempotent.
* feed credit is durable.
* multi-threshold backlog feeding works.
* remainder accounting is correct.
* `FEED_UNKNOWN` is fail-safe.

### Core Lightning

* daemon uses CLNRest.
* daemon does not access `lightning-rpc`.
* daemon has a restricted non-spending rune.
* spending RPCs are verified denied.

### OpenHAB

* automatic feeder integration works.
* `FeederOverride` works.
* manual feeding remains in OpenHAB only.
* no public manual feeder route exists.

### Overlay

* production overlay uses `lightning-goatsd`.
* one normalized WebSocket carries payment/message/feed state.
* browser receives no Lightning credentials.

### Nostr

* `nak` is used for connectivity.
* main project identity signs through NIP-46.
* project nsec is absent from `lightning-goatsd`.
* Nostr outbox is durable/idempotent.
* Zap receipt identity is separate.

### Server

* nginx is the public TLS/reverse-proxy boundary.
* `clnaddress`, `lightning-goatsd`, and CLNRest bind locally.
* bunker systemd unit is hardened.
* daemon systemd unit is hardened.
* secrets use protected credentials.

### Migration

* LNbits remained authoritative until cutover.
* production opening credit was imported correctly.
* late legacy LNbits settlements were reconciled.
* reconciliation grace period completed.
* LNbits was removed only after audit completion.
* CyberHerd remains offline.
* no herd splits occur.

---

# Part XXIII — Phase 2 Boundary

Phase 2 begins only after Phase 1 has been stable in production.

Phase 2 will separately address:

* CyberHerd port to Rust;
* Nostr engagement tracking;
* NIP-57 CyberHerd admission logic;
* member lifecycle;
* headbutt rules;
* daily resets;
* missed-event recovery;
* payout accounting;
* evaluation of an existing CLN split/payout plugin;
* restricted payout authority;
* CyberHerd overlay/member data.

No Phase 2 payout authority should be added to the Phase 1 daemon merely for convenience.

---

# Final Implementation Sequence

1. Inventory/freeze production.
2. Modify `santyr/clnaddress`.
3. Test `clnaddress`.
4. Build Rust domain and ledger.
5. Build feed state machine.
6. Add CLNRest watcher.
7. Create/test restricted rune.
8. Add OpenHAB adapter.
9. Add overlay WebSocket/API.
10. Add message templates.
11. Add Nostr transactional outbox.
12. Add `nak` NIP-46 adapter.
13. Deploy NIP-46 bunker unit.
14. Deploy `lightning-goatsd` unit.
15. Add nginx canary routes.
16. Create `herd-canary` Lightning Address.
17. Run real canary/shadow tests.
18. Build legacy LNbits reconciliation tool.
19. Prepare fresh production database.
20. Write/finalize cutover checklist.
21. Enable OpenHAB override.
22. Record cutover boundary/opening credit.
23. Disable old Lightning Goats/CyberHerd side effects.
24. Switch nginx Lightning Address routes.
25. Activate production daemon.
26. Verify a real herd payment.
27. Release OpenHAB override.
28. Observe automatic backlog draining.
29. Run legacy LNbits reconciliation through grace period.
30. Perform final audit.
31. Stop and remove LNbits runtime.
32. Freeze Phase 1.
33. Begin Phase 2 as a separate initiative.
