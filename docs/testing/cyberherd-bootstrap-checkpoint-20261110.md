# CyberHerd offline initialization checkpoint — 2026-11-10

This advances inactive installation in the full twelve-part port. No new service,
identity, credential, journal or configuration is installed on the actual host.
No payment, grant, public message, feeding or network/cutover action is activated.

Verified code is `ad4a5c500ca28fd42690591c682446e93da9f7ed`, tested as merge
`64ec4e3273159b90f5fa5d041ba3ab04294c9628`. GitHub head/merge Git-data trees and
the local feature tree match `ec05eb64175c2fe5617ee62eeeb62645a9fc668a`.
The current upstream explanation was fetched again; its raw SHA256 still matches
the pinned implementation contract, with no changed operator overrides.

[CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42805564692),
job `129793672696`, passed format, strict locked all-target/all-feature Clippy and
the full locked all-feature suite: **492 passed, zero failed, three ignored**.
The ordinary count adds an existing-file/sidecar/marker refusal regression. Two
new bootstrap parent/child fixtures are explicitly ignored in that ordinary run;
the existing isolated real-nak test is the third ignored test. These ignored
fixtures are not claimed as ordinary positive authority evidence.

The required separate root harness selects the actual compiled native library
and runs each exact fixture in a disposable network namespace, including ignored
tests only for its exact selected names. It positively requires
`ROOT_OWNERSHIP_FIXTURE_VERIFIED`, `ROOT_FOUR_JOURNAL_FIXTURE_VERIFIED` and
`ROOT_OFFLINE_BOOTSTRAP_FIXTURE_VERIFIED`. The three root repeats and two inactive
example tests are **excluded** from 492. The harness retains bounded failure
stdout/stderr rather than losing the underlying native assertion.

The new root parent prepares protected synthetic configuration and invokes the
native initializer in three subprocesses with distinct non-root kernel UIDs and
primary GIDs. Each child verifies all real/effective/saved/filesystem UID/GID
fields, empty supplementary groups, zero effective/permitted capabilities and
`NoNewPrivs=1`. Credentials are absent. A success marker is required for each
child before the parent can emit its completion marker. This is actual process
and filesystem evidence, not a mocked UID in a JSON request.

Each role refuses enabled/nonzero-budget/foreign-peer configurations before
creating state. Existing empty main files, populated SQLite sidecars, rollback
journals, dangling symlinks and recovery markers are preserved and rejected.
Valid initialization publishes a private single-link image with its own UID/GID;
retry retains the exact existing bytes. The membership owner/core cursor, payout
scope/intake and signer purpose bindings exist, while ingress, grants, execution,
payments and signing work remain empty. This proves neither an installed system
account/unit nor live provider/household/network permissions.

The [offline initialization contract](../deployment/cyberherd-offline-initialization.md)
describes the three new `--initialize-state` flags. They require protected
disabled configuration, private per-runtime state under protected ancestors,
the normal singleton and zero payout/signer admission policy. They initialize
only a private scratch database, close connections, and create a synced native
SQLite 3.54.1 `VACUUM INTO` image before exclusive no-overwrite publication.
They do not create a core bridge, read a key, run nak, contact a provider, open a
public listener, migrate existing state or start a worker.

[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42805564479)
passed all five jobs. Package job `129793669633` built all release binaries and
passed **16 non-skipped CLI checks**, including protected-policy and initialization
mode conflicts. The disabled review archive has **four binaries and 101 files**,
with `activation=none`, and retains `OFFLINE-INITIALIZATION.md` alongside the
maintenance runbook and original driver provenance. Core archive/install/systemd
fixtures remain unchanged in scope; they do not qualify new actual service roles.
Browser job `129793670184` passed **28 mocked-network tests**. Nginx general
discovery reported 148 tests with 21 environment skips; its separate nine root
upgrade fixtures passed. Do not call that discovery entirely non-skipped.

[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42805564550),
job `129793669327`, passed against 315 locked dependencies and 1,430 advisories,
with unchanged provenance and actual RSA-unreachable checks. No dependency,
schema, migration or advisory waiver was added. The native engine check remains
part of the full Rust suite. This does not patch installed programs/system SQLite.
[Isolated real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42805564840),
job `129793670134`, also passed without qualifying new household clients/relays.

Keep PR111 draft and the full goal active. The next work is the reviewed inactive
installer and actual installed UID/socket/network/helper/provider/engine/
household-client/browser qualification, concrete external recovery adoption and
explicit financial activation. Existing paid state and the live phase-one
service remain unchanged by this source checkpoint.
