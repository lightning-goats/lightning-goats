# Separate CyberHerd payout credential

The receive daemon keeps its existing credential and never loads a spend key.
The separate payout role requires only:

- [`partner.receive-request.read`](https://docs.strike.me/api/find-receive-request-by-id/)
- [`partner.payment.read`](https://docs.strike.me/api/get-payment-by-id/)
- [`partner.payment-quote.lightning.create`](https://docs.strike.me/api/create-lightning-payment-quote/)
- [`partner.payment-quote.execute`](https://docs.strike.me/api/execute-payment-quote/)

Receiving permissions and webhook signing remain with the existing core. The
payout role reads original receive resources using its own key to qualify its
account/funding binding; it creates no incoming invoices or webhooks. Do not add
balance transfer/withdrawal, account-admin or generic OpenHAB authority.
The [runtime contract](../architecture/cyberherd-payout-runtime.md) describes the
separate account/fingerprint and execution-budget checks.

`prepare-cyberherd-payout-key.py` is part of the authenticated root tool bundle.
The operator-approved maintenance installed its pinned root-owned bundle at
`/usr/local/libexec/lightning-goats-upgrade-2e3b289/`. The installed convenience
command prompts privately for the key and removes its own temporary plaintext
input after importing:

```sh
sudo /usr/local/bin/install-cyberherd-payout-key
```

Alternatively, put only the API key (without quotes) in
`/root/cyberherd-payout-key.txt`, then use the file argument:

```sh
sudo chown root:root /root/cyberherd-payout-key.txt
sudo chmod 600 /root/cyberherd-payout-key.txt
sudo /usr/local/bin/install-cyberherd-payout-key /root/cyberherd-payout-key.txt
```

The argument is a private **file path**, never a key. No webhook secret is needed.
These commands install encrypted credential data only; they do not enable payouts,
contact Strike or prove key permissions. The wrapper invokes the isolated launcher
with the independently pinned tool manifest, and does not use writable checkout
code. File arguments are retained; only wrapper-created temporary input is removed.

For a separately reviewed installation, the underlying command is below.
After independent tool/source review, the operator can place the new key in a
root-owned 0600 file under protected root-owned parents, for example
`/root/cyberherd-payout-key.txt`. Keep the key out of chat, command arguments,
shell history and environment variables. The command accepts a **path**, not the
key itself; no webhook secret is needed:

```sh
sudo /usr/bin/python3 -I -S -B /ROOT_OWNED_BUNDLE/launch-vps-upgrade.py \
  TOOL_MANIFEST_SHA256 --cyberherd-payout-key \
  --key-file /root/cyberherd-payout-key.txt --import-key
```

The existing protected host credential key/store must already exist. The script
does not initialize or replace that host key. It reads a bounded private source,
normalizes only surrounding whitespace and rejects internal whitespace/control
characters before encrypting exact token bytes as `strike-payout-api-key`.
It verifies encrypted/decrypted bytes in memory, then exclusively creates the
root-owned 0600 `/etc/credstore.encrypted/cyberherd-strike-payout` file. No key is
sent in process arguments or diagnostics. A retry with the same key retains the
existing ciphertext; a different key refuses rather than rotating a durable
financial credential/account binding. Retain partial output for inspection.

Output contains only creation status, encrypted destination and SHA256 credential
fingerprint, with `permissions_checked=false`, `api_calls=0`, `payments_sent=0`
and `services_changed=false`. Use the fingerprint in disabled payout policy.
Encryption/format success does not prove provider account or permission access.
Actual source-resource reads and operator-selected controlled execution still
require later qualification; no test payment or public note is sent here.
After successful readback, dispose of the original plaintext input using the
operator's secret handling procedure; the script never deletes an input file.
