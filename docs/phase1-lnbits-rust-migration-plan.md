# Lightning Goats Phase 1 — LNbits Removal and Rust Migration Plan

**Repository:** `lightning-goats/lightning-goats`  
**Canonical LNURL plugin:** `lightning-goats/clnaddress`  
**Phase:** 1  
**Primary objective:** replace the LNbits-based Lightning Goats runtime with a standalone Rust service while leaving the existing production stack untouched until a controlled cutover.

---

## 1. Phase 1 goals

Phase 1 must provide:

- no LNbits runtime dependency after cutover;
- Rust-based `lightning-goatsd`;
- LNURL-P / Lightning Address handling through `lightning-goats/clnaddress`;
- multiple Lightning Addresses on the same Core Lightning node;
- strict attribution of `herd@lightning-goats.com` payments through server-generated invoice labels;
- durable feed-credit accounting independent of any wallet balance;
- automatic feeder control through OpenHAB;
- no public/manual feeder endpoint;
- one read-only WebSocket/API backend for the overlay;
- Nostr publishing through `nak`;
- project Nostr signing through a NIP-46 bunker;
- a restricted Core Lightning rune with no spending authority;
- reviewed tagged release binaries with SHA-256 checksums;
- CyberHerd offline until Phase 2;
- no outbound Lightning payouts/splits in Phase 1.

---

## 2. Zero-based cutover accounting

The new Lightning Goats application ledger starts at **zero**.

There is intentionally no synchronization with the old LNbits herd-wallet balance.

At the production accounting epoch:

```text
feed_credit_sats = 0
```

Only settlements observed by the new CLN watcher whose labels match:

```text
clnaddress:v1:herd:<uuid>
```

can add feed credit.

The following are explicitly outside the new accounting boundary:

- the pre-cutover LNbits herd-wallet balance;
- pre-cutover LNbits invoices;
- old LNbits invoices that happen to settle after cutover;
- payments to other Lightning Addresses;
- ordinary CLN invoices not created for the `herd` address.

Therefore Phase 1 requires **no**:

- opening-balance import;
- LNbits invoice key in the new service stack;
- pending legacy invoice snapshot;
- late-invoice reconciliation;
- migration timer;
- migration grace-period accounting.

The old LNbits database may remain read-only for historical audit, but it has no authority over the new feed-credit ledger.

---

## 3. Non-goals

Phase 1 does not include:

- CyberHerd member management;
- CyberHerd admission/headbutt logic;
- CyberHerd payout calculation;
- outbound Lightning splits;
- replacement of the operator's OpenHAB manual feeder controls;
- a public administrative UI;
- generalized CLN administration;
- broad Lightning spend authority.

Phase 2 will address CyberHerd and constrained payout/split functionality separately.

---

## 4. Target architecture

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
       /invoice/*                         /healthz
                |                                 |
                v                                 v
          +------------+                  +-------------------+
          | clnaddress |                  | lightning-goatsd |
          | CLN plugin |                  |       Rust        |
          +-----+------+                  +---------+---------+
                |                                   |
                | creates invoices                  | restricted rune
                v                                   v
                       +----------------------------+
                       |       Core Lightning       |
                       |                            |
                       |      money trust boundary  |
                       +----------------------------+
                                      |
                                paid invoices
                                      |
                                      v
                              lightning-goatsd
                                  /       \
                                 v         v
                              OpenHAB     `nak`
                               feeder       |
                                            v
                                      NIP-46 bunker
                                            |
                                            v
                                  Lightning Goats Nostr key
```

Only nginx is Internet-facing.

`clnaddress`, `lightning-goatsd`, and CLNRest bind to loopback/internal interfaces only.

---

## 5. Canonical `clnaddress` contract

The canonical downstream fork is:

```text
lightning-goats/clnaddress
```

Phase 1 consumes the v1 invoice-label contract:

```text
clnaddress:v1:<user>:<uuid>
```

Examples:

```text
clnaddress:v1:herd:550e8400-e29b-41d4-a716-446655440000
clnaddress:v1:sat:...
clnaddress:v1:donate:...
```

`lightning-goatsd` must accept only:

```text
clnaddress:v1:herd:<valid-uuid>
```

for feed credit.

The plugin also owns:

- LNURL-P / Lightning Address HTTP behavior;
- per-address min/max policy;
- LUD-12 comment validation;
- NIP-57 request validation;
- Zap receipt publishing using a dedicated receipt identity;
- multi-address user registry.

Payer-provided descriptions, comments, or Nostr content must never determine feed-credit attribution.

---

## 6. Feed-credit model

The durable invariant is:

```text
feed_credit_sats =
    qualifying post-cutover herd receipts
  - confirmed automatic feeds * feeder_threshold_sats
```

For a 1,000-sat threshold:

```text
2340 sats received
feeds_due = 2
remainder = 340
```

Expected sequence:

```text
2340
  |
  +-- feed #1 confirmed --> 1340
  |
  +-- feed #2 confirmed --> 340
```

The credit is an application ledger, not a CLN wallet/subwallet balance.

---

## 7. Feeder state machine

A physical actuator cannot be transactionally atomic with SQLite.

Required flow:

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
    |                                  +---- debit one threshold
    |
    +---- ambiguous result -------> FEED_UNKNOWN
```

Rules:

1. Never debit a threshold before a confirmed feed.
2. `FEED_UNKNOWN` halts automatic feeding.
3. `FEED_UNKNOWN` is never automatically retried.
4. Local operator reconciliation may mark the attempt `fed` or `not-fed` but cannot actuate the feeder.
5. Manual feeding in OpenHAB does not change Lightning Goats credit.
6. Check `FeederOverride` before every queued feed.
7. Payments continue accumulating while override is ON.
8. New payments arriving while queued feeds drain simply increase the same ledger.
9. Successive automatic feeds use a configurable spacing delay.

Example:

```text
[feeder]
threshold_sats = 1000
inter_feed_delay_seconds = 30
```

---

## 8. Core Lightning observation

Use CLNRest with a dedicated restricted rune.

Required methods:

```text
waitanyinvoice
listinvoices
```

Do not grant:

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

The daemon must not be able to read the unrestricted `lightning-rpc` socket.

### Durable cursor

Use the returned `pay_index` from `waitanyinvoice` and persist it in SQLite.

For every paid invoice:

```text
BEGIN
  validate pay_index/payment_hash
  classify label
  optionally credit herd ledger
  record settlement/event
  advance cursor
COMMIT
```

Every paid invoice advances the cursor even when it does not qualify for herd credit.

The production cursor is initialized explicitly by the operator. The application never starts from zero automatically.

---

## 9. SQLite durability

Core tables include:

```text
cln_cursor
settled_invoices
ledger_entries
feed_attempts
event_log
message_outbox
message_cursor
```

Use:

```text
journal_mode=WAL
synchronous=FULL
foreign_keys=ON
busy_timeout=<reasonable value>
```

Enforce unique payment identity by `payment_hash` and `pay_index`.

There is one automatic feeder worker.

No secrets belong in SQLite.

---

## 10. OpenHAB boundary

`lightning-goatsd` may:

- read `FeederOverride`;
- invoke the configured automatic feeder rule;
- read required local status/telemetry.

It must not expose a public feeder-write endpoint.

Manual operation remains inside OpenHAB.

The feeder rule ID comes from local configuration and is never supplied by an HTTP request.

---

## 11. Nostr and `nak`

Use two identities.

### Main Lightning Goats identity

The project nsec lives only behind a NIP-46 bunker.

`lightning-goatsd` receives only a dedicated NIP-46 client key.

### `clnaddress` Zap receipt identity

Use a separate dedicated key solely for NIP-57 receipts.

Never reuse the project identity.

### Publishing model

```text
durable app event
    -> nak + NIP-46 sign
    -> persist exact signed event
    -> publish
    -> retry exact same signed JSON/event ID on failure
```

Shadow mode must never sign or publicly publish production events.

---

## 12. Overlay/API

Target public surface:

```text
GET /healthz
GET /api/v1/status
WS  /ws/overlay
```

The WebSocket is server-to-client only.

On connect/reconnect it emits a durable snapshot containing at least:

```json
{
  "type": "snapshot",
  "seq": 41882,
  "feed_credit_sats": 340,
  "threshold_sats": 1000,
  "feeds_due": 0
}
```

Subsequent events use monotonically increasing durable sequence numbers.

The browser never receives CLN, LNbits, OpenHAB, or Nostr credentials.

---

## 13. Releases

Because this is a single-operator deployment, package-manager integration is intentionally out of scope.

### `lightning-goats/lightning-goats`

Tagged releases should publish a small Linux release archive containing:

```text
lightning-goatsd
lightning-goatsctl
```

and a SHA-256 checksum file.

Deployment assets such as systemd units, nginx examples, configuration examples, and helper scripts remain versioned in the repository and are installed/reviewed manually.

### `lightning-goats/clnaddress`

Keep the existing upstream-style binary release workflow. No `.deb`, `.rpm`, container image, or additional package format is required.

---

## 14. systemd security boundary

`lightning-goatsd` runs as a dedicated unprivileged account.

Recommended hardening includes:

```text
UMask=0077
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
```

Secrets use systemd credentials:

```text
cln-rune
openhab-token
nostr-client-key
```

The bunker has its own `nostr-key` credential.

---

## 15. Canary/shadow deployment

Before production cutover:

1. install reviewed `clnaddress` and `lightning-goatsd` builds;
2. keep LNbits authoritative for all production Lightning Addresses;
3. create `herd-canary@lightning-goats.com` in `clnaddress`;
4. route only the exact canary address to `clnaddress` through nginx;
5. run the daemon in `shadow` mode against a separate canary database;
6. use an OpenHAB test rule/counter instead of the real feeder;
7. verify NIP-46 signing without public publication;
8. test websocket reconnect/snapshot/event replay;
9. test daemon/CLNRest/OpenHAB failures and restarts.

Required feed-credit test:

```text
payment = 2340 sats
threshold = 1000
expected test actuations = 2
expected remainder = 340
```

Also verify that payments to unrelated Lightning Addresses never increase herd credit.

---

## 16. Production zero-based cutover

### A. Freeze physical automatic feeding

Set:

```text
FeederOverride = ON
```

### B. Disable old Lightning Goats side effects

Disable the old LNbits Lightning Goats/CyberHerd messaging/actuation paths as appropriate for Phase 1.

LNbits itself may remain running temporarily for unrelated wallets/history, but it is no longer an accounting source for the new application.

### C. Establish the new CLN observation boundary

Before routing production `herd` traffic to `clnaddress`:

1. initialize the production `pay_index` cursor to an operator-reviewed current value;
2. confirm `feed_credit_sats = 0`;
3. start `lightning-goatsd` in `shadow` mode;
4. verify it is following new CLN settlements without side effects.

### D. Switch nginx Lightning Address routing

Move the production addresses from LNbits to `clnaddress`.

The first qualifying production invoice thereafter must have:

```text
clnaddress:v1:herd:<uuid>
```

This is the new accounting epoch.

### E. Verify production ingress while shadowed

Send a small real payment to:

```text
herd@lightning-goats.com
```

Verify:

```text
Lightning Address
 -> clnaddress
 -> attributed invoice label
 -> CLN settlement
 -> lightning-goatsd
 -> SQLite credit/event
 -> overlay
```

No physical feed should occur while shadow mode/override prevents it.

### F. Activate the daemon

Switch to active mode only after ingress/overlay/Nostr checks are clean.

### G. Release the OpenHAB override

When safe:

```text
FeederOverride = OFF
```

The daemon drains all complete thresholds sequentially and leaves the remainder.

---

## 17. Rollback

Before the first production `clnaddress:v1:herd:*` settlement, nginx can be returned to LNbits with no new-ledger accounting impact.

After the first production `clnaddress:v1:herd:*` settlement, rollback must preserve the new SQLite ledger. Do not attempt to recreate or synchronize its feed credit into LNbits.

If production routing is temporarily returned to LNbits after that point, payments received through LNbits are outside the new Lightning Goats feed-credit boundary unless the operator deliberately performs a separate manual accounting action. The preferred recovery is to repair the new ingress path rather than run two accounting authorities.

---

## 18. Security CI

Required Rust checks:

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo audit
cargo deny check
```

Also require:

- committed `Cargo.lock`;
- dependency review;
- branch protection;
- `SECURITY.md`;
- no secrets in tests/logs/config examples;
- release checksums;
- property/fuzz tests for payment classification and feed accounting where practical.

---

## 19. Phase 1 definition of done

Phase 1 is complete only when:

- LNbits is absent from Lightning Goats runtime dependencies;
- new accounting began from zero at cutover;
- no LNbits balance/late-invoice migration code exists;
- `lightning-goats/clnaddress` serves all required Lightning Addresses;
- `herd` payments are uniquely attributable by trusted labels;
- daemon CLN access is read-only/restricted;
- payment ingestion is durable and idempotent;
- multi-threshold feed draining and remainder accounting are correct;
- `FEED_UNKNOWN` is fail-safe;
- OpenHAB override behavior is correct;
- manual feeding remains exclusively in OpenHAB;
- the overlay uses one normalized WebSocket;
- Nostr uses `nak` + NIP-46 for the project identity;
- project nsec is absent from the daemon;
- tagged release binaries and checksums are produced;
- nginx remains the Internet-facing TLS/reverse-proxy boundary;
- systemd units and credentials are hardened;
- production canary/shadow testing has passed;
- CyberHerd remains offline and no outbound herd splits occur.

---

## 20. Phase 2 boundary

Phase 2 begins only after Phase 1 is stable in production.

Phase 2 will separately address:

- CyberHerd Rust port;
- Nostr engagement/admission rules;
- member lifecycle;
- daily resets and missed-event recovery;
- payout accounting;
- constrained CLN split/payout plugin evaluation;
- CyberHerd overlay/member state.

No Phase 2 payment authority should be added to the Phase 1 daemon merely for convenience.
