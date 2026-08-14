# Phase 1 Server Setup

This document prepares the production host for the Phase 1 Lightning Goats Rust migration without replacing the current LNbits-based production path until cutover.

## Safety rule

Until the production cutover is explicitly executed:

- LNbits remains authoritative for `herd@lightning-goats.com`;
- existing production Lightning Address nginx routes remain unchanged;
- `lightning-goatsd` runs in `shadow` mode;
- shadow mode must not actuate the feeder;
- shadow mode must not publish production Nostr events;
- only `herd-canary@lightning-goats.com` is routed to `clnaddress` during canary validation.

At cutover the new Lightning Goats feed ledger begins at **zero**. No LNbits balance or pending invoice state is imported.

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

## 2. Install reviewed release packages

Prefer the signed/checksummed native package produced by the repository release workflow.

Fedora/RHEL-family example:

```bash
sudo dnf install ./lightning-goats-<version>-1.x86_64.rpm
```

Debian/Ubuntu-family example:

```bash
sudo apt install ./lightning-goats_<version>_amd64.deb
```

If building directly from source for development/canary work:

```bash
git clone https://github.com/lightning-goats/lightning-goats.git
cd lightning-goats
cargo build --release --locked
sudo install -o root -g root -m 0755 target/release/lightning-goatsd /usr/local/bin/lightning-goatsd
sudo install -o root -g root -m 0755 target/release/lightning-goatsctl /usr/local/bin/lightning-goatsctl
```

Record the deployed commit/package version and binary hash.

## 3. Service account

```bash
sudo useradd --system --home-dir /var/lib/lightning-goats --shell /usr/sbin/nologin lightning-goats
```

Do **not** add the account to a group that grants unrestricted `lightning-rpc` access.

## 4. Non-secret configuration

```bash
sudo install -d -o root -g lightning-goats -m 0750 /etc/lightning-goats
sudo install -o root -g lightning-goats -m 0640 deploy/config.toml.example /etc/lightning-goats/config.toml
```

Keep during canary:

```toml
[service]
listen = "127.0.0.1:8787"
mode = "shadow"
```

Do not place secrets in TOML.

## 5. Restricted CLN rune

Create a dedicated rune that permits only:

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

## 6. systemd credentials

Required daemon credentials:

```text
cln-rune
openhab-token
nostr-client-key
```

Required bunker credential:

```text
nostr-key
```

There is intentionally **no LNbits credential** in the new stack.

Use the host's encrypted systemd credential workflow, for example:

```bash
sudo install -d -m 0700 /etc/credstore.encrypted
sudo systemd-creds encrypt - /etc/credstore.encrypted/lightning-goats-cln-rune
sudo systemd-creds encrypt - /etc/credstore.encrypted/lightning-goats-openhab
sudo systemd-creds encrypt - /etc/credstore.encrypted/lightning-goats-nostr-client
sudo systemd-creds encrypt - /etc/credstore.encrypted/lightning-goats-nostr.key
```

Never put secret values on command lines or in shell history.

## 7. NIP-46 client identity

Generate a dedicated client key:

```bash
nak key generate
```

Store the secret as the `nostr-client-key` systemd credential. Record only its public key for bunker authorization.

This client identity is not the Lightning Goats project identity.

## 8. `nak` bunker

Configure the non-secret bunker environment with:

```text
LG_NOSTR_CLIENT_PUBKEY=<dedicated client pubkey>
LG_NOSTR_RELAYS="<space-separated relay URLs>"
NAK_BIN=<output of command -v nak>
```

The bunker wrapper obtains the project signer key from the systemd credential, never places it on argv, and does not use `nak bunker --persist`.

Install/enable the hardened bunker unit only after reviewing the paths for this host.

## 9. `lightning-goatsd` unit

Install the hardened systemd unit and reload systemd:

```bash
sudo systemctl daemon-reload
```

Do not enable active side effects yet.

## 10. Canonical `clnaddress`

Use:

```text
lightning-goats/clnaddress
```

The required invoice-label contract is:

```text
clnaddress:v1:<user>:<uuid>
```

For production herd payments:

```text
clnaddress:v1:herd:<uuid>
```

Create a canary address first:

```bash
lightning-cli clnaddress-adduser herd-canary false "Lightning Goats Phase 1 canary"
```

Do not migrate the production `herd` route yet.

## 11. nginx canary routing

Review `deploy/nginx/lightning-goats-canary.conf.example` against the existing `lightning-goats.com` server block.

Always:

```bash
sudo nginx -t
sudo systemctl reload nginx
```

Only `herd-canary@lightning-goats.com` points at `clnaddress` during canary testing. Existing production addresses remain on LNbits.

## 12. Canary database and cursor

Canary testing uses a separate SQLite database that is never promoted to production.

Initialize its `pay_index` deliberately with `lightning-goatsctl init-cursor`; never replay the node from zero automatically.

## 13. Start bunker and daemon in shadow mode

Verify:

- both services are healthy;
- listener is loopback only;
- no physical feeder actuation occurs;
- no public production Nostr event is emitted;
- no unrestricted CLN RPC socket is accessible to the daemon user.

## 14. Canary tests

Use `herd-canary@lightning-goats.com` and an OpenHAB test rule/counter.

Required case:

```text
2340 sats received
threshold = 1000
expected test actuations = 2
expected remainder = 340 sats
```

Also test:

- 1 / 999 / 1000 / 1250 / 2000 / 10550 sats;
- concurrent settlements;
- daemon restart;
- CLNRest interruption;
- duplicate/replayed settlement;
- payment to another Lightning Address;
- ordinary non-clnaddress CLN invoice;
- malformed labels;
- override ON/OFF and override enabled during backlog draining;
- NIP-57 Zap;
- NIP-46 signing;
- websocket reconnect;
- OpenHAB timeout and `FEED_UNKNOWN` reconciliation.

## 15. Production zero-based cutover

Do this only after canary acceptance.

### A. Enable `FeederOverride`

```text
FeederOverride = ON
```

### B. Disable old Lightning Goats side effects

Disable the LNbits-based Lightning Goats/CyberHerd actuation/messaging path as appropriate for Phase 1. LNbits itself may stay running for unrelated uses/history.

### C. Initialize the production accounting epoch

With production routing still on LNbits:

1. create a fresh production SQLite database;
2. initialize its CLN `pay_index` to an operator-reviewed current value;
3. verify `feed_credit_sats=0`;
4. start `lightning-goatsd` in `shadow` mode;
5. confirm it follows new CLN settlements with no side effects.

### D. Switch nginx

Route the production Lightning Addresses to `clnaddress` and reload nginx successfully.

From this point forward, only trusted `clnaddress:v1:herd:*` settlements create Lightning Goats feed credit.

Old LNbits wallet balance and late LNbits settlements are ignored by design.

### E. Verify real production ingress

Send a small payment to `herd@lightning-goats.com` and verify:

```text
Lightning Address
 -> clnaddress
 -> clnaddress:v1:herd:<uuid>
 -> CLN settlement
 -> lightning-goatsd
 -> SQLite credit/event
 -> overlay
```

### F. Activate

Switch the daemon to `active`, then verify Nostr and overlay behavior.

### G. Release override

Set `FeederOverride = OFF` only after all checks pass. Complete thresholds drain sequentially and any remainder stays as feed credit.

## 16. Post-cutover LNbits handling

There is no migration/reconciliation grace period.

The old LNbits database can remain read-only for historical audit. Old invoices that settle there do not become new Lightning Goats feed credit.

When the operator is satisfied that LNbits is no longer needed for other purposes:

- stop/disable it;
- archive its DB/configuration as desired;
- revoke obsolete keys;
- remove obsolete nginx paths;
- remove old overlay LNbits websocket dependencies.

## 17. Do not add CyberHerd payouts in Phase 1

CyberHerd remains offline until Phase 2. No outbound Lightning spend capability belongs in the Phase 1 daemon.
