# Installed disabled-role OS qualification

`qualify-cyberherd-installed.py` is a bounded host qualification tool in the
authenticated root closure. Its [source-pinned checkpoint](../testing/cyberherd-installed-qualification-checkpoint-20261110.md)
records passing CI and the actual installed-host run. It starts only temporary Python
policy probes. The actual CyberHerd units remain masked; no application worker,
provider, relay, public listener, real API/Nostr credential or feeder is used.
The earlier `rehearse-cyberherd-boundaries.py` remains **CI-only**; do not bypass
its guard to run it on a live host.

Install a fresh independently authenticated tool bundle using
the [existing launcher procedure](inactive-upgrade-transaction.md). Keep earlier
generations intact. Select the actual accepted inactive installation transaction
and a new private qualification destination directly under the protected upgrade
base. Names accept only bounded letters, digits, dot, underscore and hyphen.

```sh
sudo unshare --net -- /usr/bin/python3 -I -S -B \
  /ROOT_OWNED_BUNDLE/launch-vps-upgrade.py TOOL_MANIFEST_SHA256 \
  --cyberherd-qualify \
  --installation /var/lib/lightning-goats-upgrades/ORIGINAL_INACTIVE_INSTALL \
  --destination /var/lib/lightning-goats-upgrades/NEW_QUALIFICATION \
  --probe-disabled-roles
```

The isolated-Python authenticated launcher and explicit flag are mandatory. Before
NSS, installed-state, service or crypto operations, qualification rejects the host
network namespace and any interface other than isolated loopback. It requires the
existing protected systemd host key; it never initializes/replaces it. Every role
must still be persistently masked, inactive/dead and PID zero, with no other
process under its UID. Configurations must match the accepted installation receipt
and remain disabled with zero financial/signing limits and no enabled messaging/
view. Existing role runtime directories, changed journals, sidecars or a recovery
marker refuse this fresh-install qualification. Do not delete legitimate data or
change masks merely to bypass these checks.

The tool authenticates the retained review archive, exact installed unit bytes,
root-owned programs/configurations, original private journal bytes/metadata and
actual NSS identities. Journals are hashed as regular files and never parsed,
checkpointed, migrated or reset. The original receive unit/configuration and
process are recorded independently. Its paid database is only statted to establish
that foreign-access denial tests target a real private file; it is not copied or
parsed while running. The command neither stops nor restarts that core.

Inside the new network namespace, root starts public/private IPv4/IPv6 mock TCP
endpoints and positively verifies all four. Three temporary copies of the exact
installed units retain their user/groups, capability, filesystem, private-state
and IP-denial policies. Only ExecStart, restart behavior, encrypted credential
source and the test network namespace change. Additional helper/credential/
environment commands refuse rather than reaching a production capability.

Known invalid synthetic tokens are encrypted **under the new private qualification
destination**, not in the real credential store. The copies never reference a
production encrypted credential. The existing systemd host key serves only this
synthetic encryption. Each Python probe checks actual UID/GID/groups, zero kernel
capabilities, NoNewPrivs, its network namespace, exact own synthetic credential,
denial of other credentials/journals and the real credential store, immutable
managed files, writable private own-state scratch and the four IP controls.
Temporary unnamed scratch writes and uniquely named ready/result markers do not
open financial journals. Each result must contain every required positive field.

Cleanup stops/removes only the exact uniquely named probe units and marker files.
Changed unit files are retained for inspection. Application masks, programs,
configurations and journal bytes must match their initial baseline afterward;
the original core unit/configuration/process must also match. Success retains a
root-private baseline, synthetic ciphertext and `RESULT.json`. Failure retains
`INCOMPLETE.json` and available private probe diagnostics; never interpret a partial
directory as acceptance or erase it to reuse a transaction name.

This qualifies **installed OS policy with Python probes**. It does not prove the
actual Rust socket/worker protocol, production DNS/recipient policy, global host
firewall, provider permissions/transfers, household kinds, browser behavior or
recovery adoption. Its explicit result flags keep those distinctions. No result
authorizes publication, nonzero budgets, unmasking, real payment or feeding.

The disposable systemd installer rehearsal now invokes this exact authenticated
tool after fresh native initialization. Six local guard/unit/result-contract
tests cover refusal and exact policy preservation; they simulate no live host
success. Exact CI and actual-host evidence are recorded separately in the checkpoint.

The separate [native worker fixture](cyberherd-worker-qualification.md) prepares
actual Rust socket/restart checks with synthetic journals and zero limits. It
does not replace or broaden this earlier OS-probe result.
