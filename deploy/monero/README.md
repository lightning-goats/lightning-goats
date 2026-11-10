# Monero bridge: prepare first, activate separately

Issue #97. Read [the contract](../../docs/architecture/monero-bridge.md) before
using these files. Every command in this guide is a template for the home agent;
shipping this directory does not run it or authorize live changes.

The existing OpenHAB/weather gateway, payment ledger, nginx sites, WireGuard hub
and port 5000 are not replaced. This is a new dedicated view-only receiver path.
No automatic package manager, wallet creation, account creation, secret generation,
firewall application, service startup or enablement is included in the installer.

## 1. Build and stage an approved source

From the reviewed project checkout with its committed lockfile:

```sh
cargo build --release --locked --bin lightning-goats-monero-bridge
sha256sum target/release/lightning-goats-monero-bridge
python3 deploy/monero/prepare.py stage \
  target/release/lightning-goats-monero-bridge APPROVED_SHA256 NEW_STAGING_DIRECTORY
```

Supply the digest of the approved build, not a guessed digest. `stage` verifies
that exact binary and emits `SHA256SUMS` covering its copied files. It refuses an
existing output. Review the source commit, dependency audit and applicable tests
as well: a matching self-generated checksum alone does not establish provenance.
The existing normal release archive is not yet extended to contain this binary.

## 2. Inventory and render private host values

First inspect existing listeners and service identities on the actual home host.
Choose six distinct ports (bridge API, callbacks, MoneroPay, wallet RPC, node RPC,
HTTPS/mTLS). Port 5000 must not be selected for any of them. The samples below
are unreserved examples, not instructions to move existing listeners.

Fill a private JSON file containing exactly these fields. Replace the fingerprint
with SHA256 of the dedicated account-0 primary address, obtained/verified locally.
Never put a wallet address, key or credential in an issue/PR comment.

```json
{
  "account_scope": "REPLACE_WITH_64_LOWERCASE_HEX",
  "network": "mainnet",
  "bridge_uid": 999,
  "home_ipv4": "10.8.0.6",
  "vps_ipv4": "10.8.0.12",
  "bridge_port": 18991,
  "callback_port": 18992,
  "provider_port": 18993,
  "wallet_port": 18994,
  "daemon_port": 18081,
  "mtls_port": 18995,
  "max_amount_atomic": 1000000000000,
  "server_cert": "/etc/lightning-goats-monero/tls/server.crt",
  "server_key": "/etc/lightning-goats-monero/tls/server.key",
  "client_ca": "/etc/lightning-goats-monero/tls/project-client-ca.crt"
}
```

Use the actual dedicated non-root service UID and actual VPS identity, not 999 or
10.8.0.12 merely because they appear in this example. The renderer intentionally
rejects other VPN ranges until their policy is reviewed. Then:

```sh
python3 deploy/monero/render-config.py PRIVATE_VALUES.json NEW_RENDERED_DIRECTORY
/path/to/reviewed/lightning-goats-monero-bridge \
  --config NEW_RENDERED_DIRECTORY/config.toml --check-config
```

The second command validates syntax/policy only: it does not open a database,
read credentials or contact a provider. The renderer writes new mode-0600 files
under a mode-0700 directory, with `create_enabled=false`. It emits bridge config,
sync-probe config, a complete nginx vhost snippet and the UID egress rule. It does
not test occupied ports, install certificates or apply the files.

Review resource-policy sample values. In particular, calculate actual maximum
poll delay against receipt freshness and quote lifetime as retained intents grow.
Do not silently delete paid/expired mappings when a cap is reached.

## 3. Inactive installation

An operator-approved home session must establish the dedicated service account
`lightning_goats_monero` with no shell/admin role and verify existing paths are
not another service's state. Put the reviewed staging bundle under a root-owned,
non-group/world-writable directory and ancestors. Do not run an unreviewed script
as root from a service-writable checkout or `/tmp`.

Run only after those checks:

```sh
sudo bash /ROOT_OWNED_APPROVED_STAGING/install-inactive.sh --install-new-inactive
```

This installs only new dedicated `/opt`, `/etc`, `/var/lib` and `/etc/systemd/system`
paths. It refuses existing state/installations/units and verifies bundle hashes.
It does **not** run daemon-reload/start/enable, operate MoneroPay or change networking.
A failure after creating some paths requires inspection; it does not erase paths
or perform an automatic destructive rollback. This is not an upgrade script.

The templates intentionally cannot run unchanged: the real config, identity and
credentials are absent. Place reviewed rendered configs separately, with root
ownership, no service write permission and appropriate read access. The sync probe
requires root-owned mode-0600 JSON configuration and credentials. The bridge's
systemd unit loads its separate API token from:

```text
/etc/lightning-goats-monero/credentials/api-token
```

Generate that random token privately on the approved host; do not use test tokens.
The wallet RPC credential is a different root-owned mode-0600 JSON file with
`username` and `password`. It is only read by the separate sync producer. Neither
wallet RPC credentials nor the OpenHAB token belong in the bridge environment.

## 4. Home acceptance, before exposing new receive creation

In a specifically approved live session, verify the actual MoneroPay binary/API
pin and its wallet/node settings. Keep MoneroPay and wallet RPC loopback-only.
Use a dedicated view-only project wallet; verify subaddress creation/monitoring
with that chosen stack, retaining the spend key elsewhere. Do not assume a hot
wallet is safe merely because the bridge exposes no transfer route.

The sync probe's local node must report online synchronized state, peer evidence,
correct network and a wallet scan height within the explicit lag policy. Confirm
it really is the wallet/node used by MoneroPay. A healthy HTTP response alone is
not sufficient. Probe errors emit an unavailable report or allow an old report
to expire; they must not be overridden with a hand-written `synchronized:true`.

Review the proposed dedicated mTLS vhost, client CA, UID egress and complete
home/hub policy. Validate nginx/nft syntax in isolation before applying approved
changes. Require rejection of absent/wrong client certificates and bearer tokens,
wrong peer, callback/spending/admin paths, direct wallet RPC, direct MoneroPay and
unrelated LAN access. Neither these policy tests nor actual systemd sandbox
acceptance are established by the offline Python tests.

The units have no `[Install]` target; activation is explicit. The sync producer
must first have a protected report directory and current valid report. Start the
bridge with new creation still disabled and verify protected health/status behavior.
Any later enablement, certificate/network change, real receive or XMR payment is
a separate operator-run acceptance step, not an effect of this installation.

## 5. Backup, rollback and restore

Stop all bridge writers before a coordinated backup. Back up the VPS quote/credit
ledger and the separate MoneroPay database/wallet scan state consistently too.
A bridge-only backup is not a full financial or wallet recovery package.

```sh
python3 deploy/monero/prepare.py backup SOURCE_BRIDGE.sqlite3 NEW_BACKUP.sqlite3 \
  --ack-quiesced
python3 deploy/monero/prepare.py prepare-restore BACKUP.sqlite3 NEW_CANDIDATE.sqlite3 \
  --ack-quiesced
```

The helper refuses existing outputs/SQLite sidecars, checks source/target integrity,
uses SQLite backup (not a bare copy omitting WAL), and hashes the completed file.
`prepare-restore` preserves receive history but rotates generation and puts intents
on `restore_review_required`. It does not replace a live database. There is no
automatic hold-clear endpoint. #98 must reject an unexpected bridge generation
and coordinate receipt/credit recovery before dispatching any newly funded feed.
Never restore an older paid ledger simply to retry a deployment.

Ordinary rollback disables only new Monero receive creation/ingress, keeps address
mappings and reconciliation alive when safe, and leaves Strike and the physical
owner untouched. Stop exposure before removing the network identity/containment
that protects it. Do not flush the home firewall or delete a wallet/database.

## Offline checks

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --test monero_bridge
cargo test --locked --all-features
python3 -m unittest discover -s deploy/monero -p 'test_*.py' -v
bash -n deploy/monero/install-inactive.sh
```

These tests use temporary files, mock HTTP and synthetic reports. The installer is
syntax/guard-tested, not positively installed on a real host in this implementation
session. The separate workflow runs the Python preparation tests; standard Rust CI
runs the new Rust suite. Do not report unrun GitHub checks as passed.

## Handoff to #98 / #99

Return only the authenticated private bridge contract to the VPS adapter. Do not
proxy it to the public browser or relay its raw data to Nostr. #98 creates one quote,
reserves one bridge ID, durably binds one verified address, and reconciles complete
fresh receipt history before calling the credit ledger. Never retry an uncertain
creation with a new ID as a recovery shortcut.

#99 handles BTC sats-only and non-BTC native-plus-credited-sats announcements using
explicit public fields. No address, tx hash, wallet identifier or capability may
leak through that presentation. This bridge does not implement those messages.
