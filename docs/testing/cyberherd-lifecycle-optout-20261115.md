# Automatic lifecycle broadcasts disabled — 2026-11-15

The operator rejected automatic feeder-pause Nostr announcements, citing event
`8384b211f5b554aaf52b24e7848494fc14e2c9d8609763fbed02d1cb733d3610`.
The existing `[messaging] lifecycle = false` setting now disables automatic
pause/resume broadcasts. Stream chat remains false. Member notes, payment/feeding
messages and the independent OBS paused/queued status remain enabled.

The runtime guards both lifecycle observation and the notice-delivery task with
this setting (`runtime.rs`, `delivery_runtime.rs`). Thus new lifecycle notices
are not created and retained notices are not selected for retry while disabled.
Existing signed records and relay receipts were retained without editing journals.
Re-enabling lifecycle deliberately permits retained retries; do not do so without
a new operator instruction. The supplied old event was not deleted or retracted.

## Applied and verified

The exact installed member binary SHA256 was
`8616193750221e6db9de2551b16aaca1387db1e8f1f5a5e4cd54e2f4492907bf`.
Its native parser accepted the candidate with networking isolated before any
service change; `--check` exits before opening journals/loading credentials.
Parsed TOML comparison permitted only `messaging.lifecycle: true -> false`.
A root-private reversible one-file transaction preserved ownership/mode/SELinux
attributes. Only `cyberherd.service` was stopped/restarted; other three service
states/PIDs and protected programs/configurations were guarded unchanged.
No schema, credential, payment, feeding command or manual journal edit occurred.

Configuration SHA256 before:
`4d016b1552478680c764f46607d554bc9efcfea82ce6b067f70d65fa245b3f03`.
Configuration SHA256 after:
`231615128af5ddba761e3e5bd80f55c8150422f3258abefe09e41b79cc55d859`.
Protected result:
`/var/lib/lightning-goats-upgrades/lifecycle-optout-20261115-final-review/RESULT.json`,
SHA256 `f20f01ef5ee756fcd5aff72868718610f5c6ba7b7000122102ce8a4c85151668`.

Two preparation attempts stopped before changing files/services: an assumed
unit path differed from the actual `/usr/local/lib/systemd/system` location;
the subsequent invocation omitted the transaction primitive's required guard.
Both failed attempts were retained. The corrected guarded transaction applied.

After restart, live configuration readback confirmed lifecycle false, member
notes enabled and stream chat false. Core health and member roster API returned
HTTP 200; membership was active/running with zero automatic restarts and exit
status zero. These observations do not claim a new paid message or feeding test.
This configuration-only opt-out needs no Rust build or schema maintenance.
