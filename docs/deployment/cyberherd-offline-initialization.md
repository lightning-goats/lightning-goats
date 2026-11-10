# Offline initialization before CyberHerd activation

The three CyberHerd daemons provide `--initialize-state` for a **fresh disabled
installation**. This creates only schema, selected owner/core/provider bindings,
membership settings and default templates. It is not migration, financial
recovery, historical intake, credential installation or service activation.
The feature is under source verification and is not installed by this document.

First provision the reviewed root-owned binaries/configuration, three separate
non-admin identities and each identity's own private state directory. Select
the existing core UID/epoch, explicit intake cutoff, project owner, provider
account/fingerprint and separate peer IDs in the protected configuration. Never
guess these values or invent another epoch to bypass existing financial claims.
Keep service units inactive/masked during provisioning, with no other writers.

The initializer requires canonical protected root-owned configuration and state
ancestors, its separate non-root runtime UID, a non-root primary group and a
private state directory belonging to that UID/group. It takes the same kernel
singleton as normal startup. It rechecks the protected configuration before
publication. The membership process and optional messaging/view must be disabled.
Payout process/execution and all monetary caps must be zero/disabled. Signer
process/admission/net limits must likewise be zero/disabled.

Only after that inactive provisioning, run each command as its own runtime user:

```sh
sudo -u cyberherd /usr/local/bin/cyberherdd \
  --config /etc/cyberherd/config.toml --initialize-state
sudo -u cyberherd-payout /usr/local/bin/cyberherd-payoutd \
  --config /etc/cyberherd-payout/config.toml --initialize-state
sudo -u cyberherd-signer /usr/local/bin/cyberherd-signerd \
  --config /etc/cyberherd-signer/config.toml --initialize-state
```

No credential directory, API key, nak binary/client key, provider call, core
socket, public listener, relay connection, feeder request or normal worker is
needed. `--check` remains configuration-only and cannot be combined with
initialization. Payout's existing failed-cost synchronization is also a separate,
conflicting operation; initialization cannot acknowledge/release an old hold.

Any existing destination database, even an empty file or dangling symlink, fails
before initialization. Existing WAL, SHM, rollback journal or
`CYBERHERD_RECOVERY_REQUIRED` marker also fails. The initializer never opens or
migrates that selected existing state. Do not delete paid journals or sidecars
to make this command succeed; use the reviewed recovery procedure instead.

New schemas and metadata are built in a newly created private scratch directory.
After all initialization transactions finish and connections close, native
SQLite 3.54.1 writes a complete `VACUUM INTO` image containing committed WAL
pages. The image is private, synced and published through an exclusive hard link
to the absent final pathname, never an overwriting rename. The temporary link
is removed and the state directory synced before success. This requires a local
filesystem with working hard links and fsync, as does the installed-state test.

Failure before publication leaves the original final path absent and may leave
only this invocation's private `.initialize-*` scratch directory for diagnosis.
A late filesystem failure can leave the final initial image present; repeat then
refuses it. Preserve and inspect that image rather than silently replacing it.
Successful cleanup removes only this invocation's scratch tree; a cleanup warning
does not erase or recreate the published journal. Existing state is never cleaned
up by this command.

The resulting journals have empty payment/grant/execute/signing/ingress work and
their original selected bindings. They can be included in the
[four-journal maintenance set](cyberherd-state-maintenance.md). Initialization
does not confirm actual provider permissions, authorize a bunker client, install
units, change limits, start services or grant financial/physical activation.

Source tests include ordinary existing-file/sidecar/marker rejection, CLI conflict
and protected-policy checks, plus a mandatory isolated root fixture that invokes
native initialization under three distinct non-root kernel UIDs/GIDs with cleared
groups and no credentials. Its child and parent fixtures are explicitly ignored
in the ordinary suite and are required separately by CI with positive completion
markers. It checks disabled/zero/foreign-UID refusals before creation, private
single-link ownership, exact no-overwrite retry, selected bindings and empty
financial/signing work. These are not actual installed service or live network,
provider, household-client, browser or recovery-adoption qualification.
