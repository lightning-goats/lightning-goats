# Actual core transaction rehearsal — 2026-11-10

Verified code `2e3b289f4de907855668583e6dd9e5af37081024` and tested merge
`5a9e367c46529a65ce59ed693e168efefca8bff1` share tree
`06fa6abf750b46a86a70d9ccad2b79e4ab1cc1a3`.
This extends the [inactive installer checkpoint](cyberherd-installation-checkpoint-20261110.md)
with the complete core prepare/apply/finish transaction on a disposable systemd
VM. No actual live-host account, service, configuration, credential, payment,
grant, public post, feeding or network state changed.

[CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42825925880),
job `129863074338`, passed format, strict locked all-target/all-feature Clippy and
the locked all-feature suite: **495 passed, zero failed, four ignored**. All four
required separate root markers passed. Root repeats and inactive example tests
remain excluded from 495.

[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42825925713)
passed all five jobs. Its first systemd attempt failed before the rehearsal when
GitHub temporarily could not find the just-uploaded artifact. The artifact's exact
name/source/digest were confirmed. The integration lacked retry permission;
authenticated `gh` retried the failed job without a code/guard change. Final
systemd job `129865362491` passed the complete transaction and subsequent actual
three-role disabled installation.

The new synthetic historical journal has one paid credit, a pending feeder UUID,
the original epoch and valid signed outbox bytes. It is created as the non-root
core UID under `/run`, then copied into an explicitly empty disposable core state
directory. Existing journals refuse this fixture. Old binary stand-ins and a
non-admin **sleep** unit make real replacement/stop/mask/restore observable without
starting the payment daemon or contacting a provider/gateway/relay.

The actual authenticated coordinator verifies the archive and original baseline,
preserves the custom unit, stops/masks it, accepts a native old-store backup before
binary replacement, and migrates using actual `setpriv` UID/GID/groups/caps/NNP.
The native post-migration comparison positively preserves old rows/IDs/signatures
and epoch. Both installed binary hashes match the selected release. Finish
restores exact original unit/configuration bytes and verifies inactive/dead/PID
zero; database ownership remains with the original core UID. It does not restart.

The resulting private receipt reports `actual_core_upgrade_transaction=true`,
three actual NSS identities/masked units/native initializers, SQLite 3.54.1,
`payment_daemon_started=false`, `cyberherd_units_started=false` and
`runtime_credentials_loaded=false`. A harmless fixture unit did start; it is not
reported as a payment daemon. The payout token remains synthetic and root-only.
Provider, household and actual live-host qualification remain false.

Artifact `13770245964`, ZIP SHA256
`1d52c2e5c10debc7c5bdf66539d522c4892bd1cae98ec36bbea2dca251bc86d8`,
was downloaded through the connector, its digest checked and receipt inspected.
The complete source-pinned thirteen-file tool bundle remains exported as data,
without root installation or execution on the live host.

[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42825925852),
job `129863074419`, and
[isolated real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42825925838),
job `129863074556`, also passed. No dependency, schema, advisory waiver or live
authority was added by this rehearsal extension.

The live core upgrade requires the explicit operator maintenance step; it remains
unchanged pending that decision. Separate actual payout scope/credentials,
household clients, installed sandbox/socket/network/helper/browser checks,
independently reconciled recovery adoption and explicit financial activation
remain required. Keep PR111 draft and the full twelve-part goal active.
