# Private operational alert preparation

Production HOLD. The commands and unit below are candidates for a reviewed
installation/session manifest, not authority to provision production credentials,
start the worker, publish a DM or perform a sweep. The financial daemon and HOME
owner remain separate. The operator performs manual sweeps.

## Runtime and preparation commands

The existing packaged `lightning-goatsctl` binary provides:

- `private-alert initialize`: create a new private SQLite store using only the
  protected policy/runtime credentials; refuses existing state. No provider or
  signer credentials, network clients or subprocesses are used.
- `private-alert check`: open and validate existing private state against its
  protected configuration, without provider or signer credentials or publication.
  This checks local state, not remote credential scope or signer capability.
- `private-alert run`: require existing state and all four dedicated credentials,
  then run the balance observer and ciphertext publisher until SIGTERM/SIGINT.

These commands are dispatched before loading the application's `--config` or
opening its financial ledger. Errors printed by the CLI are generic; do not
respond by enabling private-value debug logging. `initialize` is not a restore
or reconciliation command. Missing state never automatically bootstraps on run.

## Protected material

All four inputs use the service manager's absolute `CREDENTIALS_DIRECTORY`.
Each injected file must be a regular non-symlink 0400 file, no larger than 16 KiB.
Key contents must additionally be nonempty UTF-8 and at most 4096 bytes after
trimming. Source plaintext belongs in a separately approved secure provisioning
path; never put it in a checkout, command argument, GitHub or Hexmem.

| Credential | Contents and authority |
| --- | --- |
| `private-alert-policy` | Strict JSON: `threshold_sats`, lowercase-hex `recipient`, explicit `inbox_relays` array, reviewed `account_binding` generation label |
| `private-alert-runtime` | Strict JSON: `database_url`, `strike_api_url`, absolute `nak_path`, absolute `nak_config_path`, lowercase-hex `bunker_pubkey`, explicit `bunker_relays` array |
| `private-alert-strike-key` | Dedicated authoritative balance-read credential; verify effective account and `partner.balances.read` scope, with no spending authority |
| `private-alert-nostr-key` | Dedicated NIP-46 client authorization; the project signing private key stays in the separate signer |

There are no provider/signer credential fallbacks to the financial daemon's
names, and no DM-relay fallback to announcement relays. Inbox metadata discovery
is a separate reviewed preparation action, never part of worker polling.

The state binding covers the policy plus provider URL, signer public identity
and signer relay list. Changing any of these stops startup until reconciliation.
The account-generation label and binding cannot prove which actual provider
account a token controls: independent effective-scope/account verification is
still required. This is also not binary/source attestation.

A reviewed deployment normally assigns the database to
`sqlite:///var/lib/lightning-goats-alert/alerts.db`, the nak scratch directory to
`/run/lightning-goats-alert/nak`, and the executable to `/usr/local/bin/nak`.
These paths are not defaults: the protected runtime credential must state them.
Use separate temporary paths, synthetic identities and loopback provider/relay
endpoints for tests. Never use a production policy or credential in synthetic
acceptance.

## Installation and acceptance sequence

1. Pin the combined reviewed source/release and verify every archive checksum,
   provenance record, CLI executable and pinned nak hash. The archive verifier
   requires the private-alert unit. Preserve failed-run evidence.
2. Prepare a dedicated non-admin `lightning-goats-alert` identity and review the
   unit, directory ownership and encrypted credential assignments. The supplied
   `deploy/systemd/lightning-goats-private-alert.service` is an inactive candidate;
   installation does not include enablement or startup. Do not reuse the daemon
   or HOME runtime identity.
3. Verify in an isolated rehearsal that the *actual* unit gets only its four
   credentials, with effective ownership/mode; cannot read daemon/bunker/HOME
   credentials or financial state; cannot write installed binaries/configuration;
   and cannot reach household/private ranges. The unit's IPAddressDeny directives
   are proposed controls, not proof of effective kernel or final egress policy.
   Existing daemon/gateway sandbox evidence does not cover this new identity.
4. Initialize only a reviewed new store using a disposable preparation service
   with the same dedicated identity/directories, only policy/runtime credentials
   and `PrivateNetwork=yes`. Its sole command is the packaged CLI's `private-alert
   initialize`. Repeating initialization must fail. Review the concrete service
   invocation and backup/disposition before a host maintenance window.
5. Run the offline `check` under equivalent preparation isolation. Verify that
   missing state, changed policy and changed provider/signer assignment fail.
   No provider/signer key is needed for steps 4 and 5.
6. In an approved synthetic rehearsal, run the actual CLI/unit with mock provider,
   synthetic signer/recipient and local relay. Count wrapping and every publication;
   test high/low/rearm, unavailable provider, unavailable signer, failed relay,
   restart and shutdown. Validate plaintext only using the synthetic recipient.
7. Only after separate operator approval, provision verified operational material
   and perform recipient-client delivery acceptance. Record the accepted inbox
   relay set and actual delivery observation. No public event or real payment is
   part of that DM acceptance. Activation remains a distinct decision.

The API/CLI tests exercise the real command with temporary files, a harmless
HTTP provider and an explicitly non-cryptographic nak stub. Crypto acceptance is
separately established with the pinned real bunker. Neither substitutes for
steps 3, 6 or 7 using the shipped unit and accepted operational configuration.

## Stop and restore

Stop the worker before backing up or replacing its state. Allow up to the unit's
240-second stop timeout for bounded nak/SQLite cleanup. Confirm the process and
children are gone before copying/checkpointing the database. Preserve the SQLite
state consistently with its WAL, protected policy/runtime generation and source
manifest; do not copy a live main database file alone.

Keep alert state separate from the financial/gateway stores, and include it in
the reviewed paired-restore inventory. Restoring an old snapshot may forget a
previously completed high-balance episode. The current implementation has no
independent full-host anti-rollback witness. Therefore restore stays inactive
until the operator/reviewer reconciles episode state and configuration; do not
silently delete/reinitialize state or auto-rearm. Run offline `check` after a
reviewed restoration, then obtain the required activation approval. A passing
check proves binding/schema agreement, not absence of rollback.

## Disposable systemd rehearsal

`deploy/scripts/rehearse-private-alert.py ARCHIVE SOURCE` requires root solely for
transient systemd/ownership setup, an already initialized staging host credential
key, and a fresh network namespace containing only loopback. It verifies the
archive first and runs the packaged CLI only as an existing non-root identity.
It never creates users, enables services, reads production credentials or joins
the household network. Deployment CI supplies the exact tested package through
Actions artifact transfer and retains JSON plus failure logs.

The harness preserves the shipped alert Service sandbox properties, substituting
unique paths, synthetic encrypted credential sources and the existing `daemon`
identity. It joins only its isolated mock-provider namespace. Preparation uses
oneshot initialize/check with just policy/runtime credentials; runtime retains
Type/Restart behavior. A temporary RemainAfterExit setting permits inspection of
successful SIGTERM exit before explicit stop/collection. The harness fails on
any automatic restart. A distinct `nobody` unit remains active with its own
synthetic encrypted credential while the alert unit proves cross-unit denial.

The actual-unit probe verifies injected credential owner/mode/read-only access,
non-root identity, no effective capabilities, NoNewPrivileges, code nonwritability,
state writability, encrypted-source inaccessibility and network namespace identity.
The loopback provider requires the synthetic balance token; a non-cryptographic
nak process fixture counts one wrap and one publication across worker restart.
Plaintext source credential files are removed before services start. Cleanup
stops and collects every unit before removing its state/code directories.

This is preparation evidence only. Mock encryption is not the real-bunker proof;
temporary identities are not final account provisioning; a loopback namespace is
not proof of final public-relay/private-network egress policy. Those acceptance
gates remain separate even when the rehearsal passes.
