# Nostr delivery hardening: isolated parallel work

Base: PR #54, `74d4333d8b89179f68cf3fb004230d6c256ddb69`.
Branch: `parallel/nostr-delivery-hardening-20260911`.
Production HOLD remains. No merge, live signer, relay publication, payment,
physical action, server installation or network change is part of this work.

## Division of work

Codex retains the owner completion/serialization correction and home-side
containment policy. This change touches only `src/nostr.rs`, the new offline
Nostr tests/fixture, and this note. No dependency, migration, gateway, OpenHAB,
firewall, website or deployment-unit changes are required by this slice.
Integrate the focused commit after reviewing its diff; do not replace the current
Codex branch with this parallel branch or replay the earlier PR stack.

## Changes

- Require the returned signed event's content and tags to match the request,
  in addition to the existing pubkey/kind checks and `nak verify` invocation.
- Require successful publication output to echo the exact persisted event;
  an altered/missing echo is not marked published. Retries still use the same
  stored event rather than signing a new one.
- Bound JSON input and stdout to 64 KiB each (input gets one newline); stderr
  to 8 KiB. Drain both output pipes concurrently with stdin and process exit.
- Apply the existing 45-second limit to stdin, output and exit together, with
  at most five additional seconds for child kill/reap after failure. Retain
  kill-on-drop for cancellation; service-cgroup cleanup remains systemd's job.
- Withhold raw stderr and deserialization diagnostics, which can contain echoed
  credentials. Errors retain the operation, limit/timeout or process exit status.
  This deliberately trades raw subprocess diagnostics for secret-safe logs.

These checks constrain the CLI boundary; they do not make a compromised `nak`
binary trustworthy or prove relay retention. Confirm the deployed, pinned `nak`
version's event echo/exit behavior in separately authorized staging acceptance.

## Regressions and evidence

Run the focused checks from the integrated source tree:

```sh
cargo test --locked --test nostr_delivery
cargo test --locked --lib nostr::tests
```

The fixture uses `/usr/bin/python3`, temporary private files, fake event signatures
and a synthetic client key. It never opens a network connection. Tests exercise
real subprocess pipes and the actual message processor/outbox, but are not
cryptographic or live NIP-46/relay tests. The subprocess cases target Unix;
Linux additionally checks that a timed-out child has been reaped.

Nine integration cases cover exact message/credential separation, changed signer
content/tags, credential-echo errors, bounded stdout/stderr, changed publish echo,
verification failure, unchanged cursor on signing failure, overlay-only audience,
and same-event outbox retry after a failed publication and database reopen.
Three additional unit cases cover stdin-inclusive timeout, pre-spawn input
rejection, and exact output-limit boundaries; two existing Nostr unit cases remain.

Local before/after evidence used the retained audit scratch source. Its original
`src/nostr.rs` blob `126b6f03bc4bf987701d629fe61d6e42d93b28fc` exactly matches
PR #54. Five new integration cases failed before the patch; all nine pass after
it, along with all five Nostr unit cases. That is focused local evidence, not a
clean checkout of the whole current stack. Exact-head GitHub CI, Security and
deployment checks must pass before accepting the integrated candidate.

## Remaining gates

Real signer/relay compatibility, full overlay/browser acceptance, owner finality,
home-side containment and production approval remain separate. Keep #6/#15/#16
open. No physical request can be inferred or authorized by a messaging test.
