# Phase 1 Server Setup

This document prepares the production host for the Phase 1 Lightning Goats Rust migration without replacing the current LNbits production path until cutover.

## Safety rule

Until the production cutover is explicitly executed:

- LNbits remains authoritative for `herd@lightning-goats.com`;
- existing production Lightning Address nginx routes remain unchanged;
- only `herd-canary@lightning-goats.com` is routed to `clnaddress`;
- the canary daemon uses its own SQLite database and loopback port;
- canary mode may invoke only a harmless OpenHAB test/counter rule;
- canary mode never initializes the NIP-46 signer and never publishes Nostr;
- the physical feeder remains under the existing production system until cutover.

At production cutover the new feed-credit ledger begins at **zero**. No LNbits balance or pending invoice state is imported.

## 1. Record current host state

Archive before changes:

```bash
sudo nginx -T > /root/nginx-pre-lightning-goats-migration.conf
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

Do not copy secrets into the report.

## 2. Install reviewed release binaries

Download the tagged Linux release artifacts and `SHA256SUMS`.

```bash
sha256sum -c SHA256SUMS
sudo install -o root -g root -m 0755 lightning-goatsd /usr/local/bin/lightning-goatsd
sudo install -o root -g root -m 0755 lightning-goatsctl /usr/local/bin/lightning-goatsctl
```

The release also contains a tarball with both binaries. Deployment assets remain versioned in Git and are copied manually.

Record the deployed tag/commit and binary hashes.

## 3. Create the service account

```bash
sudo useradd --system --home-dir /var/lib/lightning-goats --shell /usr/sbin/nologin lightning-goats
```

Do **not** add this account to any group that grants unrestricted `lightning-rpc` access.

## 4. Install configuration and units

```bash
sudo install -d -o root -g lightning-goats -m 0750 /etc/lightning-goats
sudo install -o root -g lightning-goats -m 0640 deploy/config.toml.example \
  /etc/lightning-goats/config.toml
sudo install -o root -g lightning-goats -m 0640 deploy/config.canary.toml.example \
  /etc/lightning-goats/config.canary.toml

sudo install -o root -g root -m 0644 deploy/systemd/lightning-goats.service \
  /etc/systemd/system/lightning-goats.service
sudo install -o root -g root -m 0644 deploy/systemd/lightning-goats-canary.service \
  /etc/systemd/system/lightning-goats-canary.service
sudo systemctl daemon-reload
```

Production begins in `shadow`; canary uses `canary`.

```toml
# /etc/lightning-goats/config.toml
[service]
listen = "127.0.0.1:8787"
mode = "shadow"

# /etc/lightning-goats/config.canary.toml
[service]
listen = "127.0.0.1:8788"
mode = "canary"
```

The canary config **must** use:

```text
herd_user = herd-canary
separate SQLite DB
harmless OpenHAB feeder_rule_id
```

Never point `config.canary.toml` at the physical feeder rule.

## 5. Create the restricted CLN rune

Create one dedicated rune that permits only:

```text
waitanyinvoice
listinvoices
```

Confirm exact `createrune` syntax against the installed CLN release.

Explicitly verify that at least these methods are denied:

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

The daemon uses CLNRest and must not read the unrestricted Unix RPC socket.

## 6. Install systemd credentials

Both production and canary require:

```text
cln-rune
openhab-token
```

Only production active mode requires:

```text
nostr-client-key
```

The NIP-46 bunker service separately receives:

```text
nostr-key
```

There is intentionally **no LNbits credential** in the new stack.

Example encrypted credential setup:

```bash
sudo install -d -m 0700 /etc/credstore.encrypted
sudo systemd-creds encrypt - /etc/credstore.encrypted/lightning-goats-cln-rune
sudo systemd-creds encrypt - /etc/credstore.encrypted/lightning-goats-openhab
sudo systemd-creds encrypt - /etc/credstore.encrypted/lightning-goats-nostr-client
sudo systemd-creds encrypt - /etc/credstore.encrypted/lightning-goats-nostr.key
```

Never place secret values on command lines or in shell history.

## 7. Prepare the NIP-46 identity for production

Generate a dedicated client key:

```bash
nak key generate
```

Store the secret as the `nostr-client-key` systemd credential. Record only its public key for bunker authorization.

The client identity is not the Lightning Goats project identity.

Canary mode does not read this credential. Test NIP-46 signing separately without publishing before production activation.

## 8. Prepare the `nak` bunker

Configure the non-secret bunker environment with:

```text
LG_NOSTR_CLIENT_PUBKEY=<dedicated client pubkey>
LG_NOSTR_RELAYS="<space-separated relay URLs>"
NAK_BIN=<output of command -v nak>
```

The bunker wrapper obtains the project signer key from a systemd credential, never places it on argv, and does not use `nak bunker --persist`.

The bunker does not need to run for canary payment/feed testing.

## 9. Deploy canonical `clnaddress`

Use:

```text
lightning-goats/clnaddress
```

Verify the release checksum before installing the optimized plugin binary.

Required invoice-label contract:

```text
clnaddress:v1:<user>:<uuid>
```

Create the canary address:

```bash
lightning-cli clnaddress-adduser herd-canary false "Lightning Goats Phase 1 canary"
```

Do not migrate the production `herd` route yet.

## 10. Install nginx canary routing

Review `deploy/nginx/lightning-goats-canary.conf.example` against the existing `lightning-goats.com` server block.

It exposes only:

```text
/.well-known/lnurlp/herd-canary -> clnaddress
/invoice/...                    -> clnaddress callback
/canary/api/v1/status           -> 127.0.0.1:8788
/canary/healthz                 -> 127.0.0.1:8788
/canary/ws/overlay              -> 127.0.0.1:8788/ws/overlay
```

Before reload:

```bash
sudo nginx -t
sudo systemctl reload nginx
```

Production `herd@lightning-goats.com` remains on LNbits.

## 11. Initialize the canary database

Canary uses:

```text
/var/lib/lightning-goats/lightning-goats-canary.db
```

Determine an operator-reviewed current CLN `pay_index`, then initialize the canary DB exactly once using the canary config:

```bash
sudo -u lightning-goats \
  /usr/local/bin/lightning-goatsctl \
  --config /etc/lightning-goats/config.canary.toml \
  init-cursor --pay-index <CURRENT_PAY_INDEX>
```

Never initialize from zero automatically.

## 12. Start the canary daemon

Verify the configured OpenHAB rule is a harmless counter/test rule first.

```bash
sudo systemctl enable --now lightning-goats-canary.service
sudo systemctl status lightning-goats-canary.service
curl --fail https://lightning-goats.com/canary/healthz
curl --fail https://lightning-goats.com/canary/api/v1/status
```

Expected status includes:

```text
mode = canary
herd_user = herd-canary
feed_credit_sats
threshold_sats
feeds_due
remainder_sats
feeder_override_active
temperature_f (when configured)
```

Confirm the canary process does not have a `nostr-client-key` credential.

## 13. Use the canary overlay

Use the Phase 1 overlay with:

```text
progress3.html?canary=1
```

It connects only to:

```text
https://lightning-goats.com/canary/api/v1/status
wss://lightning-goats.com/canary/ws/overlay
```

The same overlay without `?canary=1` is the production configuration.

## 14. Canary acceptance tests

Use `herd-canary@lightning-goats.com` and the OpenHAB test counter.

Required case:

```text
2340 sats received
threshold = 1000
expected OpenHAB test-rule executions = 2
expected remainder = 340 sats
expected public Nostr events = 0
```

Also test:

```text
1
999
1000
1250
2000
10550 sats
```

Verify:

- payment to another Lightning Address adds zero herd credit;
- ordinary CLN invoice adds zero herd credit;
- duplicate settlement never double-credits;
- daemon restart preserves cursor/ledger;
- CLNRest interruption recovers without replay;
- override ON blocks all rule invocations while preserving credit;
- override OFF drains each complete threshold sequentially;
- enabling override during backlog drain stops before the next feed;
- OpenHAB timeout/error produces `FEED_UNKNOWN` and no automatic retry;
- operator reconciliation behaves correctly;
- WebSocket reconnect begins from a durable snapshot;
- a deliberate event-sequence gap forces overlay resync;
- feeder animation appears only after `feeder_confirmed`;
- canary never publishes Nostr.

Archive the canary DB/results. Never promote the canary DB to production.

## 15. Prepare production shadow

After canary acceptance:

1. create a fresh production SQLite DB;
2. initialize its CLN cursor to an operator-reviewed current value while routing still uses LNbits;
3. verify `feed_credit_sats = 0`;
4. start `lightning-goats.service` with `mode = shadow`;
5. verify it follows CLN settlements with no feeder or Nostr side effects;
6. verify production `/api/v1/status` and `/ws/overlay` locally before nginx cutover.

## 16. Production zero-based cutover

### A. Enable `FeederOverride`

```text
FeederOverride = ON
```

### B. Disable old Lightning Goats side effects

Disable the LNbits-based Lightning Goats/CyberHerd actuation and messaging paths. LNbits itself may remain running for unrelated uses/history.

### C. Switch nginx Lightning Address ingress

Route production Lightning Addresses to `clnaddress` and reload nginx successfully.

From this point forward, only trusted:

```text
clnaddress:v1:herd:<uuid>
```

settlements create new Lightning Goats feed credit.

Old LNbits wallet balance and late LNbits settlements are ignored by design.

### D. Verify one real production payment while shadowed

Send a small payment to:

```text
herd@lightning-goats.com
```

Verify:

```text
Lightning Address
 -> clnaddress
 -> attributed label
 -> CLN settlement
 -> SQLite credit/event
 -> production overlay
```

No physical feed or Nostr publication occurs in shadow.

### E. Start production NIP-46 services

Start/verify the bunker and switch the production daemon configuration to `active` only after signer connectivity is confirmed.

### F. Release override

Set:

```text
FeederOverride = OFF
```

Complete thresholds drain sequentially and any remainder remains as feed credit.

## 17. Post-cutover

There is no LNbits accounting migration or reconciliation grace period.

The old LNbits database can remain read-only for historical audit. Old invoices that settle there do not become new feed credit.

When LNbits is no longer needed for anything else:

- stop/disable it;
- archive its DB/configuration as desired;
- revoke obsolete keys;
- remove obsolete nginx paths;
- remove any legacy deployment artifacts.

CyberHerd remains offline until Phase 2. No outbound Lightning spend capability belongs in the Phase 1 daemon.
