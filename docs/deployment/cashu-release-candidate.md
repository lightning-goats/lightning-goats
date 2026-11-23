# Cashu release candidate

Production remains on hold. Build the review candidate from a clean source checkout.
The archive contains the core daemon and CLI, feeder gateway, wallet daemon,
Cashu signer and wallet state tool. It includes the inactive wallet and signer
unit examples, shadow core configuration, nginx example, operator runbooks,
backup tools, the Cashu overlay and its assets, and the locked Rust dependency
notices. The bundled QR code library includes its MIT notice and source record.
Provider credentials, wallet state and private keys are provisioned separately
through the approved procedures.

```sh
cargo fetch --locked
cargo fetch --locked --manifest-path tools/cashu-compat/Cargo.toml
python3 deploy/scripts/package-cashu.py build /tmp/lg-cashu-review
(
  cd /tmp/lg-cashu-review
  sha256sum --check SHA256SUMS
)
python3 deploy/scripts/package-cashu.py verify /tmp/lg-cashu-review/*.tar.gz "$(git rev-parse HEAD)"
```

Use Rust and Cargo 1.88, a native x86-64 GNU/Linux host, GNU binutils, a C
compiler, Python 3.11 or later and the cached locked crates. The builder uses
`--release` by default through Cargo's release profile. Supply `--profile dev`
for a local build rehearsal. Both profiles compile offline after the cache is
populated. The deployment workflow assembles the release profile from the
selected Git checkout. Run the verifier against the independently selected
source commit and artifact checksum.

`BUILD-INFO.json` records the source tree, tool versions and executable hashes.
Each executable records Cargo packages whose archive members GNU ld selected,
Rust toolchain and system archive members, the ELF shared-library requirements
and the highest required numeric glibc symbol version. The archive selection
table may include objects whose sections the linker subsequently discarded.
Build-only and other locked packages retain notice coverage in the complete
offline export. The packaged Rust toolchain notices accompany the selected
standard library code. The host supplies system libraries. Use the existing
source-pinned procedures for external tools such as `nak` and `age`.

The `cashu-backup-capsule/` directory contains the source-pinned backup and
maintenance programs with their source-bound `TOOL.json`. The archive verifier
checks the capsule manifest against the selected source and its packaged tools.
Use the approved ownership and authentication procedure in the [backup runbook](cashu-backup-collection.md).
Coordinated recovery also needs the matching CyberHerd binaries from the same
workflow checkout and the verified external `age` executable.

The builder runs `--help` on each trusted build and writes the archive with
ordered members, normalized modes and Git-commit timestamps. The tar payload
reassembles deterministically with the same inputs. Gzip bytes can vary with the
compression implementation. Independent compiled-binary reproducibility across
hosts remains unverified. The verifier reads the
archive directly and checks its contents, hashes, lock inventories, source
identity and ELF headers. Installed startup and live provider or feeder
acceptance follow their existing verification procedures.

The separate `cashu-systemd` job uses the same release archive in a disposable
Ubuntu 24.04 runner. It exercises the packaged wallet and signer under their
service sandbox properties with isolated paths, two synthetic runtime identities,
and a host-encrypted synthetic signer credential. Both must start and stop twice
with IP denied even to a listening loopback socket. A fenced restart must open no
IPC listener and preserve the retained state. The job uses newly initialized empty
fixtures. Funded checkpoint installation and reconciliation remain separate.

## Render the inactive Minibits configuration

Use the packaged `deploy/scripts/render-cashu-pilot.py` with the current core
TOML, a private JSON file containing the reviewed bindings, and a new private
review directory. The renderer preserves the original core bytes and appends
only the three Cashu configuration sections. It produces a core, wallet and
signer configuration plus a SHA-256 review record. All Cashu service controls
are off. Keep this review separate from installed configuration.

The binding file has these fields.

| Fields | Reviewed values |
| --- | --- |
| `version` | `1` |
| `core_uid`, `wallet_uid`, `wallet_gid`, `signer_uid`, `signer_gid` | Actual dedicated numeric UIDs and the two Cashu IPC GIDs |
| `core_epoch`, `wallet_instance`, `wallet_epoch`, `broker_instance`, `broker_epoch` | Retained core epoch and the reviewed wallet and signer registration UUIDs |
| `receiving_key`, `client_pubkey`, `helper_sha256` | Reviewed compressed receiving public key, dedicated x-only signer public key and exact installed signer helper digest |
| `strike_account_id`, `strike_credential_fingerprint` | The reviewed Strike destination account UUID and receive-only credential fingerprint |
| `daily_credit_sats`, `minimum_deposit_sats`, `relay_since_unix` | Explicit rolling credit-sat limit of at least 1,999 sats, qualified minimum Strike deposit and starting NIP-61 observation time |
| `mint_keysets` | Pinned Minibits sat records of the form `{"id":"…","keys":{"1":"…"}}`, copied from reviewed public mint evidence |
| `nutzap_relays` (optional) | Nonempty reviewed subset of the existing core Nostr relay list |

Use the existing Strike API origin and the project relay inventory. Select
`nutzap_relays` using the native read-only relay probe. The selection binds
Nutzap intake, the signer's announcement scope and the inactive Cashu
publication settings. Ordinary messaging and bunker transport retain their
existing relay configuration. Live publication still needs its own test.
Omit the selection to use the original project relay list for Nutzap intake
and signing without configuring publication delivery.

The pilot uses `https://mint.minibits.cash/Bitcoin` and batches automatic
Strike sweeps at 1,000 sats of available Cashu. The 999-sat receipt limit and
1,999-sat custody and conversion bounds give the receipt that crosses the
threshold room to be credited. The rolling credit-sat limit covers the
same headroom. Each conversion reserves up to 5 sats in fees, with a 25-sat
rolling fee ceiling. Recovered change stays in custody until the next batch.
The existing source and fee bounds still govern each conversion. The pilot
configures no fixed daily receipt or conversion count. Private keys, seed
bytes and raw Strike credentials are not inputs.

```sh
python3 -I -S -B deploy/scripts/render-cashu-pilot.py CORE_CONFIG BINDINGS REVIEW
```

Validate the exact reviewed files with the selected native programs under the
actual service identities. The validators require root-owned protected paths.
Use the normal private configuration placement for each account.

```sh
lightning-goatsctl --config REVIEW/core.toml check-config
lightning-goats-cashud check-config REVIEW/wallet.json
lightning-goats-cashu-signerd check-config REVIEW/signer.json
```

The core `check-config` command parses startup settings without opening or
migrating the database. These checks do not load wallet keys, load the signer
credential, contact Strike or publish. Generate and review the reusable
descriptor from the bound wallet configuration before enabling HTTP intake.
Provider reachability, key permissions, activation and phone acceptance remain
separate observations.

To record the selected request in an inactive review, first generate its preview
with the selected native wallet binary using the exact private wallet
configuration, public HTTPS endpoint and chosen encoding. Pass that
`prepare-request` JSON and the same endpoint when rendering a fresh review.

```sh
python3 -I -S -B deploy/scripts/render-cashu-pilot.py CORE_CONFIG BINDINGS REVIEW \
  --reviewed-request HTTPS_ENDPOINT PREVIEW_JSON
```

The output registers the descriptor ID, selects the endpoint and encoding, and
records the original preview digest in `REVIEW.json`. Receiving stays off.
Run the native wallet validator on the newly rendered file and repeat
`prepare-request` against it. Require the same ID, encoding and case-sensitive
payload, with `descriptor_registered` now true. Native validation binds the ID
to the actual mint, receiving key and endpoint. This completes the inactive
review stage while phone and provider acceptance remain pending.

## Core and wallet Unix sockets

Install these two root-owned files together during the approved Cashu core
configuration step. Keep the existing core unit and its CyberHerd drop-ins.

| Packaged example | Installed path |
| --- | --- |
| `deploy/systemd/lightning-goats-receive.service.d/80-cashu-receipts.conf.example` | `/etc/systemd/system/lightning-goats-receive.service.d/80-cashu-receipts.conf` |
| `deploy/tmpfiles.d/lightning-goats-cashu-receipts.conf.example` | `/etc/tmpfiles.d/lightning-goats-cashu-receipts.conf` |

Provision the dedicated `lg-cashu-ipc` group and wallet account first. The
tmpfiles rule creates `/run/lightning-goats-cashu-receipts` as
`lightning-goats:lg-cashu-ipc` with mode `0750`. Use
`systemd-tmpfiles --create --prefix=/run/lightning-goats-cashu-receipts
/etc/tmpfiles.d/lightning-goats-cashu-receipts.conf` before the approved
core startup so its mount sandbox finds the runtime path. The drop-in
adds the group and the writable socket directory to the core service. The
main core runtime and the original CyberHerd bridge keep their own groups
and paths.

Set `cashu_receipts.socket` in the core and `receipt_delivery.socket` in
the wallet to `/run/lightning-goats-cashu-receipts/receipt.sock`. Set the
core's receipt IPC GID and the wallet's listener IPC GID to the numeric
`lg-cashu-ipc` group. Use `/run/lightning-goats-cashu/intake.sock` and
`/run/lightning-goats-cashu/treasury.sock` for the wallet's separate
listeners. Keep the wallet-to-signer socket in the separate
`lg-cashu-signer-ipc` group.

The packaged systemd rehearsal starts the native core with the packaged
receipt permissions mapped into disposable paths. It exercises a synthetic
wallet-UID receipt, identical acknowledgment on replay, refusal of the
signer UID even when granted the socket group, and one ledger credit with
zero feed attempts. The original payment event keeps its advisory feeding
snapshot. Receipt verification checks the original financial fields
independently so a presentation update cannot block import or replay.

## Add the inactive Cashu schema to an existing core

Use the authenticated root-owned VPS upgrade tool closure with the paired core
release and CyberHerd review archives from the same verified source commit.
Pass their independently verified SHA-256 digests. A Cashu upgrade preserves
the original configuration, including protected systemd drop-ins for existing
roles, and the original active service set. The Cashu wallet and broker must
be absent or persistently masked. The initial core configuration must have no
Cashu policy.

The launcher accepts these Cashu operations:

```text
--cashu-core-prepare CORE_ARCHIVE REVIEW_ARCHIVE SOURCE CORE_SHA256 REVIEW_SHA256 REVIEW_PATH
--cashu-core-maintenance plan REVIEW_PATH EXECUTION_PATH --review-sha256 PREPARED_SHA256
--cashu-core-maintenance apply EXECUTION_PATH --exclusive-maintenance
--cashu-core-maintenance resume EXECUTION_PATH --approve-resumption
```

Preparation and planning preserve the running services. Application stops and
masks the four journal writers, takes a fresh native schema-25 snapshot, and
migrates a separate clone under the original runtime UID with networking
isolated. The native comparison requires unchanged original rows, row IDs,
schemas and signed bytes, plus the compiled empty Cashu schema. Application
then installs the selected programs, migrates the fenced original and repeats
the native comparison. An interrupted migration retains its evidence and stays
fenced. Resumption verifies the same comparison and restores only the original
active roles in service dependency order, keeping the Cashu units inactive.
