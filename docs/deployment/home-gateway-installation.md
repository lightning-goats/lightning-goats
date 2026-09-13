# Home gateway installation and recovery

This home-only workflow stages production inactive and tests a harmless loopback
canary. Production HOLD and the gates in `home-gateway-agent-handoff.md` remain.
The original checkout and earthship-ui owner source are not modified.

## Fresh install

Run in a reviewed clean worktree with Rust 1.88.0, exact-source CI/Security and
`cargo fmt`, locked strict Clippy, all-feature tests and release gateway build.
Record the full source commit and `sha256sum target/release/lightning-goats-gateway`.
The installer accepts the binary bytes only when the supplied digest matches;
it does not independently certify CI, provenance or architecture.

```sh
python3 deploy/scripts/prepare-home-gateway.py --binary target/release/lightning-goats-gateway --sha256 <verified-digest>
sudo python3 deploy/scripts/prepare-home-gateway.py --binary target/release/lightning-goats-gateway --sha256 <verified-digest> --apply
systemd-analyze verify /etc/systemd/system/lightning-goats-gateway{,-canary}.service
```

The helper refuses existing accounts/groups/project files/units/state and unsafe
installation parents. It creates two locked, non-login system users with only
their private primary groups. Binary, configs and units are root-owned; `/etc`
configuration directories remain root-managed rather than systemd-owned by the
runtime user. Both listeners are loopback, production 8789 and canary 8790.
The helper never enables or starts either unit. Partial failures stay inactive;
inspect what was created before preparing an exact recovery, never blindly rerun
with overwrite flags. Existing deployments need a separate upgrade plan.

Production retains `feeder_request_v1`, 30-second minimum and 10/hour cap pending
operator sizing. Canary uses only `LightningGoatsCanary*` Items, `uuid_canary`,
5-second minimum/ack timeout and 60/hour cap. Its database and service account are
separate. Unit credentials remain named `openhab-token`, with distinct ciphertexts:
`/etc/credstore.encrypted/lightning-goats-gateway-openhab` and
`/etc/credstore.encrypted/lightning-goats-gateway-canary-openhab`.
No optional temperature Item is configured until an explicit unit is available.

## Harmless OpenHAB fixture

```sh
python3 deploy/scripts/prepare-home-openhab-canary.py --provisioning-env <protected-local-env>
python3 deploy/scripts/prepare-home-openhab-canary.py --provisioning-env <protected-local-env> --apply
```

Use a separate existing administrator provisioning identity. The helper refuses
existing Items, links, rule references and the same rule UID. Review the complete
live registry and file-defined rules for generic side effects before applying.
It creates five unlinked/un-grouped Items and rule
`lightning_goats_gateway_canary`; remote-enable stays OFF. The rule only updates
Count and Ack and validates a canonical UUID. It counts every command, including
invalid and duplicate commands, using a cached atomic counter so asynchronous
Item updates do not hide sequential duplicates. After rule reload/restart the
counter seeds from the Item; it is a test observation, not an authoritative
physical journal. Do not reset it during a gateway restart/replay test.

OpenHAB 5.2.1 adds empty action `inputs` on readback; verification accepts exactly
that harmless addition and rejects extra behavior. Any failed apply requires
read-only inspection; do not delete/recreate existing resources to make it pass.

## Credential provisioning

Use OpenHAB's supported `openhab:users add <user> <password> user` and
`openhab:users addApiToken <user> <label> ''` console commands. Verify the installed
version's accepted scope; never substitute admin or invented Item scopes.
Production identity/label: `lightning_goats_gateway` / `lightninggoatsgateway`;
canary: `lightning_goats_gateway_canary` / `lightninggoatsgatewaycanary`.
OpenHAB 5.2.1 rejects hyphenated token labels; the documented example labels
were corrected to supported alphanumeric names without changing ciphertext paths.
Use a protected local console credential and a non-logged input channel. Capture
new token output in process memory and pipe directly to `systemd-creds encrypt
--name=openhab-token`; do not put passwords/tokens in argv, logs, Git or chat.
Never remove or rotate another account's token. Verify exact USER account role,
valid-token Item read, harmless canary command, and denied admin GET. Record
implicit-role unauthenticated behavior separately.

The home host had no credential key and both encrypted stores were empty. The
existing guarded `initialize-staging-credential-store.py` initialized a new
root-owned mode-0400 key; no key was replaced. This host's filesystem is not
encrypted. Host-key encryption protects service credential delivery but does not
protect against offline access to both that key and ciphertext. Never regenerate
the key for token rotation. Provision a replacement token under a new label,
verify it, encrypt to a new exclusive file and review the canary-only restart;
revoke the retired label only after readback. Production remains inactive.

The repeatable `provision-home-openhab.py` helper defaults to plan and accepts
a root-owned private console-password file plus pinned known-hosts. Its supported
Karaf `shell:source` file is temporary, private, and removed in finally; passwords
never enter SSH argv or interactive history. Token output goes directly into
host-encrypted credentials after USER role and effective read checks. It refuses
existing project accounts/ciphertexts. `--resume-empty-project-users` is only for
an inspected partial run with exactly USER roles, zero tokens and zero sessions;
it does not change passwords or remove tokens. Remove temporary console inputs
after provisioning. This needs local `pexpect`, not a new daemon dependency.

After credential validation, start only `lightning-goats-gateway-canary.service`.
`sudo python3 deploy/scripts/check-home-canary.py --provisioning-env <protected-local-env> --apply` verifies
exact rule/Item bindings, remote-OFF no-dispatch, immutable refusal replay, one
confirmed UUID with duplicate and restart replay, and concurrent distinct UUIDs.
Only the canary remote switch is temporarily enabled and returned OFF in finally.
The helper never targets TCP8789 or changes the real FeederOverride.

All home HTTP helpers disable proxies and reject redirects, including same-origin
redirects. Direct permission responses remain 200/401/403. The checker requires
the canonical loopback installation: parsed configuration must exactly match the
home installer, including OpenHAB origin `http://127.0.0.1:8080/`, database and
canary bindings. Installed config/unit and parents must be root-owned and not
group/other writable. The unit must match the generated unit exactly, with no
effective drop-ins, environment files or pending daemon reload. A changed
installation needs separate review; the checker does not normalize it silently.

Before any gateway POST, the checker restarts only the verified canary to load
the checked configuration, then verifies its actual executable, command line,
runtime identity and ownership of the sole loopback listener. It repeats those
checks after the replay restart. It also fetches the full rule inventory and
rejects any other rule referencing a canary Item, including JSON-escaped names.
Operators must exclude concurrent administrative edits throughout the test and
review generic/dynamic rule consumers that cannot be proven by literal-reference
scanning. Root is required for the process and socket checks.

## Read-only synthetic credential rehearsal

`sudo python3 deploy/scripts/rehearse-home-gateway.py --apply` creates a fresh
separate validation unit on 127.0.0.1:18790 with a synthetic invalid token. It
GETs health, safety, optional temperature and real local weather; no command
POST exists in this helper. It stops/removes only its unit/config/ciphertext and
preserves a separate SQLite database plus exact backup/restore evidence under
`/var/lib/lightning-goats-gateway-validation-v2`. Existing evidence causes refusal.
A second run needs a reviewed new name, not removal of evidence.

The rehearsal rejects noncanonical installed configurations/units before creating
resources. It renders from reviewed templates, explicitly sets the validation
SQLite/config/ciphertext/state/runtime paths, and checks the derived config and
unit targets. It never copies arbitrary installed directives. Existing loaded
validation units and applicable systemd drop-ins are rejected. Alternate database
paths cannot reach SQLite initialization. The preserved v2 evidence means this
helper is not rerun against that name merely to validate a source correction.

Expected bad-token safety response is HTTP **502**, not 503. `healthz` remains
200 and does not certify credentials. Missing/empty credential startup and
concurrency/crash cases are covered by the isolated Rust/deployment fixtures.

## Rollback

Stop/disable only `lightning-goats-gateway-canary.service` if it was started in
this assignment. Preserve its entire SQLite state and hash the quiesced backup;
never restore older state over newer attempts. Remove a newly created unit or
config only after comparing its hash to the install manifest and checking for
subsequent agent changes. Keep accounts and state while evidence is retained.
The unstarted production unit can remain inactive. Remove the project canary
rule/Items through supported REST only after confirming their exact content,
unlinked status, remote OFF and absence of another consumer; never remove the
physical owner or real safety Items. Retain the host credential key. Revoke only
project tokens explicitly being retired. Do not touch OpenHAB/weather services,
old hub, household firewall, or earthship-ui checkout.

Paired daemon/gateway financial restoration remains separate acceptance. The
empty validation database backup proves local SQLite mechanics only; the Rust
fixtures carry nonempty pending/refused/restart cases. A real paired restoration
must preserve every unresolved UUID and coordinate the VPS database snapshot.

References: [OpenHAB console commands](https://www.openhab.org/docs/administration/runtime)
and [role semantics](https://www.openhab.org/docs/configuration/restdocs).
