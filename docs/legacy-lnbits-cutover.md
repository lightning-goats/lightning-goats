# Legacy LNbits Cutover and Reconciliation Runbook

This runbook covers the temporary accounting bridge from the existing LNbits-based Lightning Goats system to the standalone `lightning-goatsd` + `clnaddress` Phase 1 stack.

The goal is to preserve every sat of feeder credit without making LNbits a permanent dependency of the new service.

## Safety model

The permanent application trusts two distinct payment sources during the migration window:

1. new payments whose CLN invoice label matches the canonical `clnaddress:v1:herd:<uuid>` contract; and
2. legacy LNbits invoices whose payment hashes were captured in the stable cutover manifest while they were still pending.

The legacy bridge never guesses entitlement from LNbits timestamps. A payment hash must have been explicitly allowlisted in the stable manifest before a late settlement can create feed credit.

The opening LNbits wallet balance is imported exactly once as `LEGACY_OPENING_CREDIT`. It does not emit a synthetic payment event or Nostr message. A genuinely late allowlisted invoice emits one normal durable `payment_received` event after settlement.

The temporary LNbits migration binary:

- is not used by `lightning-goatsd`;
- receives only the legacy herd wallet **invoice key**;
- never receives the CLN rune, OpenHAB token, or NIP-46 client key;
- accepts only a localhost/loopback LNbits URL;
- performs only HTTP GET requests;
- refuses to truncate non-sat-aligned msat state;
- never overwrites an existing cutover manifest.

## Required files

Install the release binaries:

```bash
sudo install -o root -g root -m 0755 lightning-goatsd /usr/local/bin/lightning-goatsd
sudo install -o root -g root -m 0755 lightning-goatsctl /usr/local/bin/lightning-goatsctl
sudo install -o root -g root -m 0755 lightning-goats-lnbits-migrate /usr/local/bin/lightning-goats-lnbits-migrate
```

Install the temporary wrappers:

```bash
sudo install -d -o root -g root -m 0755 /usr/local/libexec/lightning-goats
sudo install -o root -g root -m 0755 \
  deploy/scripts/run-lnbits-snapshot \
  /usr/local/libexec/lightning-goats/run-lnbits-snapshot
sudo install -o root -g root -m 0755 \
  deploy/scripts/run-lnbits-reconcile \
  /usr/local/libexec/lightning-goats/run-lnbits-reconcile
```

Install the temporary units:

```bash
sudo install -o root -g root -m 0644 \
  deploy/systemd/lightning-goats-lnbits-snapshot.service \
  /etc/systemd/system/lightning-goats-lnbits-snapshot.service
sudo install -o root -g root -m 0644 \
  deploy/systemd/lightning-goats-lnbits-reconcile.service \
  /etc/systemd/system/lightning-goats-lnbits-reconcile.service
sudo install -o root -g root -m 0644 \
  deploy/systemd/lightning-goats-lnbits-reconcile.timer \
  /etc/systemd/system/lightning-goats-lnbits-reconcile.timer
sudo systemctl daemon-reload
```

Do **not** enable the reconciliation timer yet.

## LNbits invoice-key credential

The temporary tools need the invoice/read key for the **legacy herd wallet only**. Do not use the LNbits admin key.

Create an encrypted systemd credential interactively on the target host. For example:

```bash
sudo systemd-creds encrypt --name=lnbits-invoice-key - \
  /etc/credstore.encrypted/lightning-goats-lnbits-invoice-key
```

Enter the invoice key on stdin when prompted/piped from an appropriately protected source. Do not place it in shell history, Git, the environment file, or the Lightning Goats SQLite database.

The credential is temporary and must be removed after the legacy grace period.

## Temporary non-secret configuration

Create:

```text
/etc/lightning-goats/legacy-lnbits-migration.env
```

from `deploy/systemd/legacy-lnbits-migration.env.example`.

Before cutover, set only the host-local LNbits URL and snapshot policy, for example:

```text
LNBITS_URL=http://127.0.0.1:5000/
STABLE_ROUNDS=2
SNAPSHOT_INTERVAL_SECONDS=2
SNAPSHOT_MAX_ROUNDS=30
```

Leave `CUTOVER_AT` unset or as a placeholder until the production nginx route has actually moved away from LNbits.

## Pre-cutover requirements

Before touching production routing:

- `lightning-goats/clnaddress` is deployed and canary-tested;
- all required production Lightning Address users already exist in `clnaddress`;
- `lightning-goatsd` is installed and configured in `shadow` mode;
- the OpenHAB test path and overlay canary have passed;
- the production SQLite database is fresh/known and backed up;
- LNbits remains running and authoritative;
- the old Lightning Goats/CyberHerd services can be disabled without stopping LNbits itself;
- nginx pre-cutover configuration has been archived;
- rollback nginx configuration is ready.

## Cutover sequence

The order below is important.

### 1. Block physical automatic feeding

Set the OpenHAB feeder override ON:

```text
FeederOverride = ON
```

Incoming credit may continue to accumulate. Physical automatic feeder activation must remain blocked through the accounting handoff.

### 2. Disable old LNbits-based side effects

Disable the old Lightning Goats/CyberHerd/CyberHerd Messaging behavior that can feed, distribute, or publish messages.

**Keep LNbits itself running.** It must remain available to settle and report old invoices during the grace period.

### 3. Establish the CLN cursor before changing nginx

The new daemon must begin observing CLN before the first production `clnaddress` invoice can settle.

Record the current maximum paid-invoice `pay_index`:

```bash
LAST_PAY_INDEX="$(lightning-cli listinvoices | \
  jq '[.invoices[].pay_index // empty] | max // 0')"
printf 'initial CLN pay_index: %s\n' "$LAST_PAY_INDEX"
```

Initialize the production database exactly once:

```bash
sudo -u lightning-goats \
  /usr/local/bin/lightning-goatsctl \
  --config /etc/lightning-goats/config.toml \
  init-cursor --pay-index "$LAST_PAY_INDEX"
```

Start `lightning-goatsd` in **shadow** mode and verify it is healthy:

```bash
sudo systemctl start lightning-goats.service
curl --fail http://127.0.0.1:8787/healthz
sudo -u lightning-goats \
  /usr/local/bin/lightning-goatsctl \
  --config /etc/lightning-goats/config.toml status
```

Do not initialize the cursor after the nginx switch. Doing so could skip a settlement that occurred during the handoff.

### 4. Switch production Lightning Address routes to `clnaddress`

Prepare the nginx production routing changes, then:

```bash
sudo nginx -t
sudo systemctl reload nginx
```

Verify the public Lightning Address endpoints now resolve through `clnaddress`.

Immediately after the successful route switch, record the Unix cutover boundary:

```bash
CUTOVER_AT="$(date +%s)"
printf 'CUTOVER_AT=%s\n' "$CUTOVER_AT"
```

Write that exact value into `/etc/lightning-goats/legacy-lnbits-migration.env`:

```text
CUTOVER_AT=<recorded value>
```

Do not reconstruct or approximate this value later.

### 5. Capture the stable LNbits snapshot

Run the one-shot snapshot unit:

```bash
sudo systemctl start lightning-goats-lnbits-snapshot.service
sudo journalctl -u lightning-goats-lnbits-snapshot.service --no-pager
```

The snapshotter repeatedly reads the legacy herd wallet balance and pending incoming invoices. It succeeds only after the same canonical state is observed for the configured number of consecutive rounds.

The resulting file is:

```text
/var/lib/lightning-goats/legacy-lnbits-manifest.json
```

It is created mode 0600 and will not be overwritten by a subsequent snapshot command.

Inspect it before installation:

```bash
sudo jq . /var/lib/lightning-goats/legacy-lnbits-manifest.json
```

Confirm at minimum:

- `wallet_id` is the old herd wallet;
- `opening_credit_sats` matches the expected residual feeder credit;
- `cutover_at` is the value recorded after nginx reload;
- every listed pending invoice belongs to the herd wallet;
- amounts are plausible;
- there are no unexpected pending invoices.

If the snapshot is not accepted, **do not edit it in place**. Archive/remove the rejected artifact deliberately, correct the cause, and run a new snapshot so the artifact remains machine-generated and auditable.

### 6. Atomically install the manifest into the new ledger

```bash
sudo -u lightning-goats \
  /usr/local/bin/lightning-goatsctl \
  --config /etc/lightning-goats/config.toml \
  legacy-install-manifest \
  --manifest /var/lib/lightning-goats/legacy-lnbits-manifest.json
```

The command is idempotent only for the exact same manifest. A different manifest is rejected after installation.

Verify the saved JSON exactly matches the installed boundary:

```bash
sudo -u lightning-goats \
  /usr/local/bin/lightning-goatsctl \
  --config /etc/lightning-goats/config.toml \
  legacy-verify-manifest \
  --manifest /var/lib/lightning-goats/legacy-lnbits-manifest.json
```

Expected output includes:

```text
legacy_cutover_manifest=verified
```

The reconciliation wrapper performs this same verification before every run. It will fail rather than query/import LNbits if the manifest was never installed or no longer matches SQLite.

### 7. Verify shadow accounting before activation

While still in shadow mode and with OpenHAB override ON:

- send a small real payment to `herd@lightning-goats.com`;
- confirm it produces a `clnaddress:v1:herd:<uuid>` invoice;
- confirm `lightning-goatsd` credits it;
- confirm the overlay reflects the new credit;
- confirm no feeder actuation occurs;
- confirm no project Nostr message is published in shadow mode.

### 8. Switch `lightning-goatsd` to active mode

Change only the local service configuration from:

```text
mode = "shadow"
```

to:

```text
mode = "active"
```

Then restart:

```bash
sudo systemctl restart lightning-goats.service
sudo systemctl status lightning-goats.service --no-pager
```

Keep the OpenHAB override ON until the active daemon, Nostr outbox, overlay, and ledger have all been inspected.

### 9. Start temporary legacy reconciliation

First run one reconciliation manually:

```bash
sudo systemctl start lightning-goats-lnbits-reconcile.service
sudo journalctl -u lightning-goats-lnbits-reconcile.service --no-pager
```

Then enable the temporary timer:

```bash
sudo systemctl enable --now lightning-goats-lnbits-reconcile.timer
systemctl list-timers lightning-goats-lnbits-reconcile.timer
```

The timer checks only hashes captured in the cutover manifest. It does not discover or accept new legacy payment hashes after cutover.

### 10. Release the feeder override

Once the accounting boundary is verified:

```text
FeederOverride = OFF
```

The daemon drains one physical feed per complete threshold, serially.

For a 1,000-sat threshold and 2,340 sats of credit:

```text
feed #1 confirmed -> 1340
feed #2 confirmed -> 340
```

The 340-sat remainder remains credited toward the next feed.

## Why the stable snapshot is race-safe

Several races are handled deliberately:

- If an old LNbits invoice settles **before** the stable snapshot, it disappears from pending state and increases the opening balance. It is therefore represented by the opening credit, not a later import.
- If it settles **after** the stable snapshot, its hash is in the allowlist and the reconciler credits it later.
- If it settles between the wallet and payment-list reads of one snapshot round, the state changes and consecutive-round equality resets.
- If a legacy callback already in flight creates an additional LNbits invoice after nginx reload, the pending set changes and the snapshot does not stabilize until that state is visible.
- If the CLN watcher sees a late LNbits invoice before the reconciler, it records the settlement as a non-`clnaddress` invoice with zero canonical credit; the allowlisted legacy import remains valid.
- If a hash somehow already received canonical `clnaddress` herd credit, the legacy import is rejected to prevent double credit.

## Grace period

Keep all of the following during the legacy invoice expiry grace period:

- LNbits running;
- the legacy herd wallet data intact;
- the encrypted `lnbits-invoice-key` credential;
- `/var/lib/lightning-goats/legacy-lnbits-manifest.json` unchanged;
- the reconciliation timer enabled.

The timer may repeatedly report already imported, still pending, or failed invoices. Idempotency is enforced in SQLite.

Do not create new production Lightning Address invoices through LNbits after cutover.

## End of grace period

After the maximum legacy invoice lifetime plus a deliberate safety margin:

1. run the reconciliation service manually one final time;
2. inspect its output and confirm no relevant legacy invoice remains capable of settling unexpectedly;
3. archive the final migration logs/manifest with the cutover record;
4. disable and stop the timer;
5. remove the temporary LNbits invoice-key credential;
6. remove/disable the temporary snapshot/reconciliation units and environment file;
7. keep the old LNbits database/configuration as a read-only audit archive for the chosen retention period;
8. only then stop/disable LNbits runtime and remove obsolete nginx/LNbits routes.

Commands:

```bash
sudo systemctl start lightning-goats-lnbits-reconcile.service
sudo journalctl -u lightning-goats-lnbits-reconcile.service --no-pager

sudo systemctl disable --now lightning-goats-lnbits-reconcile.timer
sudo rm -f /etc/credstore.encrypted/lightning-goats-lnbits-invoice-key
sudo rm -f /etc/lightning-goats/legacy-lnbits-migration.env
sudo systemctl daemon-reload
```

Do not delete the archived LNbits database or cutover manifest as part of the initial runtime shutdown.

## Rollback boundary

Before the first production `clnaddress:v1:herd:<uuid>` settlement, nginx can be returned to LNbits and the old services re-enabled using the pre-cutover configuration.

The first confirmed production `clnaddress:v1:herd:*` settlement is the **point of no blind rollback**. After that point, CLN contains feeder credit accounted for by the new ledger but not by the old LNbits virtual herd wallet. Any rollback must explicitly reconcile both accounting systems; never simply swap them back.
