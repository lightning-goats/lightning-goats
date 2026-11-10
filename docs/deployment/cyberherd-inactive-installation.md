# Fresh inactive CyberHerd installation

The authenticated tool bundle now contains `install-cyberherd-inactive.py`.
Its source is being qualified. It is not installed or run on the live host.
Use Python **3.11 or newer** through the
[externally authenticated launcher](inactive-upgrade-transaction.md), with its
complete ten-file closure. Authenticate launcher/tool-manifest, source commit
and archive SHA256 independently before root execution. An archive's own hashes
are not provenance. Do not run a writable checkout or import archive helpers.

The new roles remain separate from the receive core. Existing paid stores and
the uploaded OBS artwork are never installed over or modified. This is a fresh
installation only, not an upgrade/restore/recovery procedure for existing journals.
Finish the [legacy-core upgrade](cyberherd-core-upgrade.md) before installing the
new units; that transaction deliberately requires them to be absent. Account
provisioning can occur earlier while new units/state are absent.

## Provision only the three identities

```sh
sudo /usr/bin/python3 -I -S -B /ROOT_OWNED_BUNDLE/launch-vps-upgrade.py \
  TOOL_MANIFEST_SHA256 --cyberherd-inactive identities --create-identities
```

This uses explicit `systemd-sysusers --inline` definitions for `cyberherd`,
`cyberherd-payout` and `cyberherd-signer`, with separate primary groups, private
state-home names, locked passwords and `/usr/sbin/nologin`. It adds only the
dedicated bridge/plans/payout-signer supplementary groups. It checks exact NSS
UID/GID/home/shell/groups, password lock and a complete sudo denial. No core user
membership, directory, configuration, credential or unit is changed. Any existing
or partial identity/group requires inspection; do not delete it to force a rerun.
The result supplies actual UIDs/GIDs for the protected configuration.

## Prepare the public disabled configuration

Resolve the three [configuration examples](../../deploy/cyberherd/) into a
protected root-owned directory as `cyberherd.toml`, `cyberherd-payout.toml` and
`cyberherd-signer.toml`. These contain policy and public identities, never keys.
Use actual NSS peer/group IDs and the original core UID/epoch from accepted
native backup evidence. Preserve one project owner and exact payout provider
account/fingerprint. Do not bind an invented key fingerprint merely to initialize
a journal; that binding persists. Actual account/credential qualification remains
required before any execution.

Keep all three `enabled=false`, membership messaging/view disabled, payout
execution disabled, every monetary ceiling zero and signer admission/rate zero.
Resolve `America/Denver`, six actual receive payees, explicit retained-event
cutoff, relays and the separate signer client public key. The client key itself
is not needed to install/initialize. Preserve optional capabilities as disabled.
Choose these values from the existing system and operator policy, not fixture
values in tests. Unknown/native-invalid fields fail before installing paths.

## Install disabled units and initialize fresh journals

Use the independently pinned four-binary CyberHerd review archive, plus the
protected core policy and accepted pre-upgrade bundle retained by the core
transaction. The installed native core CLI must already provide copy-only
`verify-core-upgrade`; the installer never uses ordinary `status` for discovery.
The upgrade base must be protected root-owned mode 0700 and the install transaction
path must be new.

```sh
sudo /usr/bin/python3 -I -S -B /ROOT_OWNED_BUNDLE/launch-vps-upgrade.py \
  TOOL_MANIFEST_SHA256 --cyberherd-inactive install \
  /PRIVATE/cyberherd-review.tar.gz /PRIVATE/resolved-configs \
  /PRIVATE/core-upgrade/policy.toml /PRIVATE/core-upgrade/legacy \
  /var/lib/lightning-goats-upgrades/NEW_INACTIVE_INSTALL \
  --source FULL_SOURCE_COMMIT --sha256 ARCHIVE_SHA256 --install-disabled
```

The installer validates absent destinations/units, identities, original native
core evidence, configuration cross-pins and zero policy. It retains an exclusive
private transaction and archive, verifies source/activation/complete checksums
and original SQLite driver provenance, then freezes selected programs/configs.
Native `--help` and `--check` paths run only after archive authentication, without
state/credentials/network. All inputs are retained before any installed path.

Persistent `/etc/systemd/system/*.service -> /dev/null` masks are established and
positively checked before publication. Root-owned programs go to `/usr/local/bin`;
root-owned configuration directories/files are 0755/0644, readable by their runtime
users and non-writable by them. Unit files go under `/usr/local/lib/systemd/system`,
with the persistent masks above them. No enable/start/unmask command exists.
Private state directories are 0700 and owned by their separate runtime UID/GID.

Each native initializer runs once as its own UID/GID with cleared supplementary
groups/capabilities and NoNewPrivs. Its previously qualified
[fresh initialization path](cyberherd-offline-initialization.md) refuses existing
main files, sidecars and recovery markers. The installer checks single-link 0600
journals, retains fingerprints and final mask/inactive/PID-zero readback, and only
then replaces `INCOMPLETE` with an `INSTALLED.json` receipt. It does not parse
original journals with system Python SQLite or load credentials.

Failure preserves the transaction and any partial masks/files/state for inspection.
It never silently reruns initialization, overwrites state, deletes a partial
journal, restores an older core or starts a unit. An existing destination refuses
a fresh invocation. Diagnose the retained phase and actual host state rather than
deleting files to bypass that refusal.

This installs **no credentials and no core bridge**. Runtime socket/firewall,
provider permissions, encrypted credentials, separate household clients/kind
permissions, effective systemd/network/helper containment and browser behavior
still require actual installed qualification. Core configuration changes,
public presentation and all financial activation remain explicit later steps.
Filesystem fixtures simulate NSS/systemd/native acceptance and must not be
reported as real installed qualification; actual native initialization has its
separate mandatory root subprocess fixture.
