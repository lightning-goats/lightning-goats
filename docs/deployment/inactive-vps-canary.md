# Prepare an inactive canary installation on a fresh VPS

Production remains HOLD. This is the new-VPS bootstrap step authorized by
`codex-vps-bootstrap.md`: create a separate runtime identity, root-managed files
and inactive staging unit. It does not replace final release review, owner/site
access, network containment or live acceptance.

The helper is deliberately for a fresh installation. It refuses existing
`lightning-goats` users/groups, project paths, active/enabled project services,
symlink/untrusted parent directories and conflicting destination files. It is
not an upgrade or repair tool. Inventory first and preserve any existing work.
Use a trusted reviewed archive and independently recorded full SHA256/source SHA;
the internal checksum manifest alone does not authenticate a download.

First verify the archive and print a concrete plan without changing host state:

```sh
python3 -B deploy/scripts/prepare-vps-canary.py \
  /path/to/trusted-release.tar.gz FULL_SOURCE_SHA ARCHIVE_SHA256
```

After reviewing the plan, apply with existing staging bootstrap privileges:

```sh
sudo python3 -B deploy/scripts/prepare-vps-canary.py \
  /path/to/trusted-release.tar.gz FULL_SOURCE_SHA ARCHIVE_SHA256 --apply
```

The helper copies the archive into private temporary storage before hashing and
extracting it, and executes no archive payload as root. It then:

- creates the system user/group `lightning-goats`, with a locked password,
  `/usr/sbin/nologin`, no supplementary groups and verified absence of sudo authority;
- installs only `lightning-goatsd` and `lightning-goatsctl` on the VPS, root-owned
  mode 0755, with checksums verified against the trusted payload;
- creates root-managed `/etc/lightning-goats` and runtime-owned private
  `/var/lib/lightning-goats`, and copies the non-secret canary **example**;
- installs the shipped `lightning-goats-canary.service` as root-owned mode 0644;
- restores default SELinux labels when applicable and verifies the unit without
  starting it or reloading the system manager;
- verifies under the actual runtime UID that code/config are readable but not
  writable, state can be written, and both installed binaries execute `--help`;
- records the source, archive/file hashes, identity and inactive status in
  `/etc/lightning-goats/STAGING-INSTALL.json` and on stdout.

No active `config.canary.toml`, credentials, home gateway binary, production unit,
SSH settings or network policy are installed. The service stays inactive and
disabled. The example retains placeholders and unaccepted owner/network bindings;
review the actual staging configuration and harmless gateway before activation.

If application fails after creating the account or some files, it stops without
enabling services. Preserve and inspect the partial state; do not rerun through
guards or delete runtime data blindly. For an entirely unused failed preparation,
remove only positively identified helper-created files and the new empty state
directory/account/group after review. An existing or nonempty ledger must be
preserved and handled through the restore/reconciliation procedure.

Final production binaries/configuration, account permissions, SSH access, DNS,
WireGuard and receive-only/signing credentials remain separate acceptance gates.
The deployed source must be updated to the final reviewed integration result
before production. Keep the current Codex/deployment account separate and revoke
its broad staging privileges before final credentials/cutover.

## Observed staging preparation, 2026-09-11

The helper was applied on Fedora 44 from the continuation based on draft #41
head `d350b2af697b0add7056a353c2fcfe97c697b8ce`. The installed archive contains
runtime source `a54ead65b7d68a402b2e4a33ff387b6198a9cf84`, SHA256
`3fe81ed9f9c9b0500425ed903cd767e9fd5d286c8d47c23530732b59a6ff924e`.
Those runtime bytes already passed the corrected systemd/mock rehearsal; the
current persistent installation itself has executed only the two help commands.

[Sanitized installation and independent readback evidence](../testing/evidence/inactive-canary-fedora-20260911.json)
records the installer hash, installed file hashes/modes, runtime UID/GID, empty
0700 state, absent active configurations/credentials, and inactive/disabled
canary. A repeated `--apply` refused the existing account and preserved all
installed file bytes and metadata. The production unit remains absent.

The first attempt stopped immediately after account creation: Fedora's sudo
returned status 0 alongside an explicit denial, while the original guard
required status 1. Read-only inspection established the account had no grants,
owned no files and had no processes. Only that newly created unused account/group
was removed; no files were deleted. The corrected guard requires the entire
C-locale denial for the exact runtime user with no diagnostics, accepting status
0 or 1. Regression tests reject grants, empty/error responses and misleading
denial substrings. The subsequent installation and readback succeeded.

The Deployment workflow also runs this fresh installation on its disposable
Ubuntu 24.04 runner after the isolated systemd rehearsal. It retains the plan
and installation JSON with checksums. Confirm that workflow's exact-head result
before accepting the Ubuntu execution; local Fedora evidence does not prove it.

The initial Ubuntu run `34549009202` passed archive and isolated systemd checks
but the fresh-install plan correctly refused runner-writable `/usr/local/bin`.
The workflow now prepares only `/usr/local` and `/usr/local/bin` as root-owned
0755 directories on that disposable runner before the positive installation
test. It does not recursively change tool ownership or weaken the installer's
parent checks. This fixture change performs no action on the Fedora VPS.
