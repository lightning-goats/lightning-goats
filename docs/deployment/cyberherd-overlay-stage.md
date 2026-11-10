# CyberHerd overlay stage preparation

This stage adds native incoming zaps, membership intake, overlay messages and a
loopback roster view while preserving the existing core's normal payment,
feeding and announcement behavior. No reward planning, payout access, delegated
feeding messages, membership Nostr signer, lifecycle notices or operator HTTP
commands are enabled by the proposed configuration. All core reward ceilings
remain zero, and both outgoing roles remain masked with their zero policies.

`stage-cyberherd-overlay.py prepare` is **preparation only**. It has no apply,
stop, start, unmask, secret-loading, database or network-configuration command.
Do not interpret a prepared bundle as authorization or an implementation of
activation. The separate [maintenance executor](cyberherd-overlay-maintenance.md)
is a source candidate awaiting installed rehearsal; no approval to run it is
requested by this document.

Use its externally authenticated root tool closure, selected through
`launch-vps-upgrade.py TOOL_SHA --cyberherd-overlay-stage`. Older protected tool
generations and the existing credential importer remain unchanged. Never run a
writable checkout as root or import a helper from a downloaded archive.

The separately pinned `CHECK-ONLY-overlay-stage` native example loads only the
two candidate configuration files. It uses the application's actual type and
policy validation, rejects financial or publication capabilities and reports no
journal, credential, service or provider operation. Authenticate its source,
artifact checksum and executable independently before installing it under a
new protected root-owned prefix. Passing parser tests alone do not prove runtime
startup or actual live behavior.

Preparation requires the accepted fresh inactive installation, unchanged sealed
role journals/configurations/programs/units and actual non-admin NSS identities.
The original running core, protected unit/drop-ins/configuration/program and
separate bridge group are pinned. Maintenance policy must retain all four actual
identities/journal paths, the original core epoch and payout provider scope; its
native check opens no state. The validator must be protected, executable and
match the independently supplied digest. Existing bridge/tmpfiles destinations
or an occupied loopback view port refuse preparation.

```sh
sudo /usr/bin/python3 -I -S -B /ROOT_OWNED_BUNDLE/launch-vps-upgrade.py \
  TOOL_MANIFEST_SHA256 --cyberherd-overlay-stage prepare \
  /PRIVATE/ACCEPTED_INACTIVE_INSTALLATION \
  /ROOT_OWNED_PREFIX/CHECK-ONLY-overlay-stage \
  /var/lib/lightning-goats-upgrades/NEW_REVIEW_DIRECTORY \
  --validator-sha256 NATIVE_EXECUTABLE_SHA256 --message-start-after 0
```

Select the message cutoff explicitly from the retained fresh domain state. It is
separate from the original core ingress cutoff, which is copied exactly and never
advanced or reset. Preparation refuses reused/partial destinations. Failed checks
leave an `INCOMPLETE` record and data for inspection; they never overwrite an
installation to make a retry pass.

Role-journal bytes are read only for the fresh-install hash comparison; the live
core journal is not opened and no journal is parsed. The assembled candidate unit
also passes `systemd-analyze verify` before preparation can be accepted.

The private bundle retains exact original configuration/unit/drop-in/policy
files, a recoverable two-configuration replacement plan, candidate core and
membership TOML, a core unit candidate and a dedicated tmpfiles candidate. The
core unit adds only the supplementary bridge group and writable socket parent;
its existing private runtime mode is preserved. The proposed parent is core-owned
0750 with the dedicated bridge group, rather than exposing credential-bearing
runtime directories. Neither the tmpfiles file nor socket parent is created by
prepare.

The candidate native zap map copies each of the six existing receive payees and
the common receipt signer, including the legacy HTTPS LNURL alias. Amount/rate/
gateway/provider/mode/address settings are preserved. Overlay delivery omits a
signer and reads no new credential. The view is loopback-only with operator routes
absent. Existing settings, core epoch/cutoff, source attribution and both outgoing
policies remain unchanged. Whole configuration is rendered with a round-trip
check; original bytes/comments remain in the protected backups.

Only after the separate maintenance executor and its installed rehearsal are
reviewed should an operator be asked to approve the brief interruption. That
executor must quiesce/mask all four writers, take and verify a current native
four-journal snapshot, preserve the custom `/etc` core unit, apply only these
changes and restore normal core operation. Never restore an older financial
database or clear replay/signed history. Public nginx/OBS integration and incoming
settlement/delivery checks remain part of the rollout; preparation installs none.
