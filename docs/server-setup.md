# Phase 1 Server Setup

This document prepares the production host for the Phase 1 Lightning Goats Rust migration without replacing the current LNbits-based production path.

## Safety rule

Until the cutover runbook is explicitly executed:

- LNbits remains authoritative for `herd@lightning-goats.com`;
- existing production Lightning Address nginx routes remain unchanged;
- CyberHerd remains live only as required by the existing stack;
- `lightning-goatsd` runs in `shadow` mode;
- shadow mode must not actuate the feeder;
- shadow mode must not publish production Nostr events;
- only `herd-canary@lightning-goats.com` is routed to `clnaddress` during canary validation.

## 1. Record the current host state

Run and archive the output before making server changes:

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

Do not copy secrets into the migration report.

## 2. Build and install `lightning-goatsd`

Build from a reviewed commit:

```bash
git clone https://github.com/lightning-goats/lightning-goats.git
cd lightning-goats
cargo build --release
sudo install -o root -g root -m 0755 target/release/lightning-goatsd /usr/local/bin/lightning-goatsd
```

Record the deployed commit and binary hash:

```bash
git rev-parse HEAD
sha256sum /usr/local/bin/lightning-goatsd
```

## 3. Create the service account

Create a dedicated account with no interactive login:

```bash
sudo useradd --system --home-dir /var/lib/lightning-goats --shell /usr/sbin/nologin lightning-goats
```

Do **not** add this account to any group that grants access to Core Lightning's unrestricted `lightning-rpc` socket.

## 4. Install non-secret configuration

```bash
sudo install -d -o root -g lightning-goats -m 0750 /etc/lightning-goats
sudo install -o root -g lightning-goats -m 0640 deploy/config.toml.example /etc/lightning-goats/config.toml
```

Edit `/etc/lightning-goats/config.toml` and keep:

```toml
[service]
listen = "127.0.0.1:8787"
mode = "shadow"
```

The application rejects non-loopback listener addresses by design.

Do not place secrets in this TOML file.

## 5. Create the restricted CLN rune

The Phase 1 application requires only paid-invoice observation/reconciliation access.

Create a dedicated rune allowing only:

```text
waitanyinvoice
listinvoices
```

Confirm the exact `createrune` syntax against the installed Core Lightning version before creating the production credential.

Before installing it, explicitly verify with `checkrune` that the rune permits the required RPCs and denies at least:

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

The daemon must access CLN through CLNRest, not the unrestricted Unix RPC socket.

## 6. Store service secrets using systemd credentials

Required `lightning-goatsd` credentials:

```text
cln-rune
openhab-token
nostr-client-key
```

Required bunker credential:

```text
nostr-key
```

The bunker `nostr-key` is the main Lightning Goats project signer key. It must not be readable by the `lightning-goatsd` service account.

Use `systemd-creds encrypt` or the host's existing encrypted credential workflow. Example shape:

```bash
sudo install -d -m 0700 /etc/credstore.encrypted
sudo systemd-creds encrypt - /etc/credstore.encrypted/lightning-goats-cln-rune
sudo systemd-creds encrypt - /etc/credstore.encrypted/lightning-goats-openhab
sudo systemd-creds encrypt - /etc/credstore.encrypted/lightning-goats-nostr-client
sudo systemd-creds encrypt - /etc/credstore.encrypted/lightning-goats-nostr.key
```

Enter the appropriate secret at each prompt. Never put the secrets into shell history or command-line arguments.

## 7. Create the dedicated NIP-46 client identity

Generate a new Nostr key used only by `lightning-goatsd` as the NIP-46 client:

```bash
nak key generate
```

Store the resulting secret as the `nostr-client-key` systemd credential.

Derive and record only its public key for bunker authorization.

The client key is **not** the Lightning Goats project identity.

## 8. Configure the `nak` bunker

Install the wrapper:

```bash
sudo install -d -o root -g root -m 0755 /usr/local/libexec/lightning-goats
sudo install -o root -g root -m 0755 deploy/scripts/run-nak-bunker \
  /usr/local/libexec/lightning-goats/run-nak-bunker
```

Install its non-secret environment file:

```bash
sudo install -o root -g root -m 0644 deploy/systemd/bunker.env.example \
  /etc/lightning-goats/bunker.env
```

Set:

```text
LG_NOSTR_CLIENT_PUBKEY=<dedicated client pubkey>
LG_NOSTR_RELAYS="<space-separated relay URLs>"
NAK_BIN=<output of command -v nak>
```

The bunker wrapper reads the project signer key from the systemd credential and exports it only to the bunker process environment. It never places the project key on argv and does not use `nak bunker --persist`.

Install the bunker unit:

```bash
sudo install -o root -g root -m 0644 deploy/systemd/lightning-goats-nostr-bunker.service \
  /etc/systemd/system/lightning-goats-nostr-bunker.service
```

## 9. Install `lightning-goatsd.service`

```bash
sudo install -o root -g root -m 0644 deploy/systemd/lightning-goats.service \
  /etc/systemd/system/lightning-goats.service
sudo systemctl daemon-reload
```

Do not enable active side effects yet.

## 10. Prepare `clnaddress`

Build the reviewed `santyr/clnaddress` fork only after the address-aware label patch is applied and its tests pass.

The required invoice label contract is:

```text
clnaddress:v1:<user>:<uuid>
```

For the production herd address:

```text
clnaddress:v1:herd:<uuid>
```

Create a canary address first:

```bash
lightning-cli clnaddress-adduser herd-canary false "Lightning Goats Phase 1 canary"
```

Do not migrate the production `herd` route at this stage.

## 11. Add nginx canary routing

Review `deploy/nginx/lightning-goats-canary.conf.example` against the existing `lightning-goats.com` server block.

Before reload:

```bash
sudo nginx -t
```

Then:

```bash
sudo systemctl reload nginx
```

Only the exact canary address should point at `clnaddress` during this stage:

```text
herd-canary@lightning-goats.com
```

All current production Lightning Addresses stay routed to LNbits.

## 12. Initialize the shadow database deliberately

The production/shadow ledger must never automatically replay the node from pay index zero.

Before starting invoice consumption, initialize its cursor to an operator-reviewed current CLN pay index. The operator tool for this step is implemented as part of Phase 1 and must refuse to replace an already-initialized cursor with a different value.

Canary testing uses a separate database that will never become the production database.

## 13. Start the bunker

```bash
sudo systemctl enable --now lightning-goats-nostr-bunker.service
sudo systemctl status lightning-goats-nostr-bunker.service
sudo journalctl -u lightning-goats-nostr-bunker.service -n 100 --no-pager
```

Confirm no project secret appears in logs or process arguments.

## 14. Start the daemon in shadow mode

```bash
sudo systemctl enable --now lightning-goats.service
sudo systemctl status lightning-goats.service
curl --fail http://127.0.0.1:8787/healthz
curl --fail http://127.0.0.1:8787/api/v1/status
```

Verify:

- listener is loopback only;
- database resides under `/var/lib/lightning-goats`;
- no feeder actuation occurs in shadow mode;
- no production Nostr event is published;
- no unrestricted Lightning RPC socket is accessible by the service account.

## 15. Canary testing

Use `herd-canary@lightning-goats.com` and a harmless OpenHAB test rule/counter.

Required accounting case:

```text
2340 sats received
threshold = 1000
expected test actuations = 2
expected remainder = 340 sats
```

Also test unrelated Lightning Addresses and ordinary CLN invoices to prove they never increase feed credit.

## 16. Do not cut over from this document

Production cutover requires the separate cutover checklist and these conditions:

- `clnaddress` label contract deployed and tested;
- Rust CI clean;
- canary/shadow accounting validated;
- OpenHAB test integration validated;
- NIP-46 signing validated;
- overlay migration validated;
- legacy LNbits reconciliation tool ready;
- opening-credit procedure ready;
- rollback boundary documented.

Until then, LNbits remains the production authority.
