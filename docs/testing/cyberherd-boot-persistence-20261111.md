# Current overlay stage boot persistence — 2026-11-11

The already running overlay-only membership service is now enabled in the actual
host's `multi-user.target`. No service was started, stopped or restarted, and no
feature, credential, financial policy, program, application configuration or
journal was changed. This records boot wiring, not an actual host reboot test.

The installed unit independently matches GitHub source at the qualified
`cade21ef0fce8eaf783c7884750943ae42056a4b` revision. Its SHA256 is
`f76258b04e1fcabccabe058aaa5b459828edbfb6b4ede31ecd32b8b4583ea378`.
Root-owned protected parents, the exact installed executable/configuration paths,
and its sole `[Install] WantedBy=multi-user.target` entry were checked before
enablement. Core was already enabled at boot; the membership unit orders itself
after core without making payment availability depend on membership health.

Actual preconditions verify the active stage: membership and readonly/OBS view
enabled; no rewards, payout bridge or member signer; lifecycle off; no delegated
feed announcement or payout execution; zero core, payout and signing ceilings;
both outgoing roles persistently masked. The operation is only:

```sh
sudo systemctl enable -- cyberherd.service
```

It deliberately omits `--now`. Independent readback verifies the root-owned
`/etc/systemd/system/multi-user.target.wants/cyberherd.service` symlink resolves
to the reviewed `/usr/local/lib/systemd/system/cyberherd.service`, the enabled
status, exact unchanged application configurations/unit and unchanged active
states, PIDs and automatic-restart counts for all four services. Payout and
signer remain masked with zero PIDs. No application journal or secret was opened.

Private before/after receipts are retained in
`/var/lib/lightning-goats-upgrades/cyberherd-boot-20261111`.
The accepted `ENABLED.json` SHA256 is
`2c485e760861f8b181bd3c4427e8b90578fe8ca270bc581dfef878d850d89b07`.
The existing [native worker restart fixture](cyberherd-worker-qualification-checkpoint-20261110.md)
separately proves the stated synthetic restart/idempotency boundary. Neither
that fixture nor this target link claims real reboot/recovery adoption, external
provider reconciliation or new financial/publication activation. The
[full port](../planning/cyberherd-implementation-plan.md) remains incomplete.
