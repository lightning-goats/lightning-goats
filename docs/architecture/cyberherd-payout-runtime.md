# Separate payout worker and outgoing signing broker

This source connects the private confirmed-feeding plan, qualified funding,
invoice/provider journal and [core outcome protocol](cyberherd-payout-outcomes.md).
It advances requirements 7–8 and part of 12 of the full port. Both new programs
and all supplied configurations remain inactive until deliberately provisioned.
There is no production installation or financial activation in this change.

## Three separate identities

`cyberherdd` exports only its earliest frozen feeding plan over an optional
kernel-authenticated Unix socket. A barrier whose plan or reservation is not
ready blocks later exports; a zero-allocation plan can advance without inventing
a reservation. Export size is capped at 4 MiB. The payout process checks the
actual intake UID and owner/cursor, then independently obtains the exact core
reservation and qualifies every consumed real receive with its own Strike key.
It cannot obtain a core database handle, feeder command or spend permission from
the intake process. A copied plan is not money authority.

`cyberherd-payoutd` owns the independent payout database and the spend credential.
It has no public listener, generic trusted-network proxy, OpenHAB token or Nostr
credential. Its intake task saves an exact receipt/cursor only after admission.
A crash between admission and cursor commit resumes the same grant. Intake API
or provider slowness cannot block the separate existing-payment recovery loop.

`cyberherd-signerd` alone loads a NIP-46 client credential for the household bunker.
The configured client public key must match that credential and differ from the
project identity. The broker never loads the herd's signing key. Its separate
database freezes the full invoice intent before calling nak and retains the
first valid signed bytes before replying. The only RPC generates that intent's
kind-9734 request. There is no arbitrary kind/content signing endpoint and no
publication endpoint; its publishing relay list must be empty. Bunker transport
relays remain independently configured.

The trusted payout UID still attests the original payee/intent. This broker does
not claim independent membership or bank-reservation verification. It enforces
the fixed event shape, project identity, configured receipt relays, per-invoice
net ceiling and bounded new-intent admission per hour. The raw request travels
only to the recipient callback. A supported recipient never silently falls back
to ordinary Lightning after signing or callback failure.

## Journal-driven work

The worker performs one durable step per tick: prepare intent, sign/freeze the
callback, fetch/validate an invoice, freeze a quote, persist/submit execute intent,
read the original payment, or report/reconcile a terminal manifest. Work selection
uses a retained increasing sequence so a held recipient/outcome cannot starve
other rows. There is one worker and one separate intake task; missed timer ticks
are skipped rather than accumulating tasks.

Callbacks, quote creation and new/recovery submission require the independent
execution switch and a fresh core permit. Supported zap callbacks additionally
require public Nostr receipt authority. Freshness is repeated at the network
boundary, and execution repeats the complete journal/budget checks. Known payment
reads and final outcome reconciliation continue with execution disabled.

A lost usable quote response can be retried with its original idempotency key.
An ID-only duplicate quote is retained and retired only if no execute intent
exists; it is never treated as validated execution authority. Expired never-sent
invoices can close explicitly. A callback that remains unresolved for an hour
may close as unsubmitted when permission resumes. A paused execution switch does
not itself discard an old reserve. After any execute intent, expiry never
authorizes a new invoice/quote/payment. Unknown submission recovers only the same
quote; known original payment IDs use GET reconciliation.

No ledger reset, carried member reward balance, feeder actuation or public treat
message occurs in this loop. Completed payouts still need the separate, frozen
member delivery path; local Completed status alone never announces a treat.

## Inactive artifacts and credentials

Review-only configuration and unit examples are under `deploy/cyberherd/` and
`deploy/systemd/cyberherd-{payout,signer}.service.example`. All runtime identities,
IPC group IDs, core epoch/account/credential pins and spend/rate limits require
explicit provisioning. Limits start at zero and execution starts false.

- Receive core: existing receive scopes plus `partner.payment.read` for its
  optional independent outgoing verifier; **no quote/execute/spend key**.
- Payout identity: `partner.receive-request.read`, `partner.payment.read`,
  `partner.payment-quote.lightning.create`, `partner.payment-quote.execute`.
  Load only as the separate `strike-payout-api-key` systemd credential.
- Signer identity: only its separate `nostr-client-key` authentication credential,
  authorized at the household bunker; no Strike credential or local project key.

Preserve the operator's existing balance ceiling/sweep policy. Config files and
programs must be root-owned and non-writable by runtime/deploy accounts. State is
private to each non-admin runtime UID. Startup checks protected configuration,
canonical state paths and a kernel singleton. Socket directories use dedicated
groups with no group-write access, and every actual caller/server UID is checked.
The examples deny trusted/private IP ranges as defense in depth; actual DNS,
cross-UID, IPv4/IPv6 and helper containment still need installation qualification.

`--check` validates configuration without creating state, connecting sockets or
reading credentials. It is not a provider permission or live-activation check.

`deploy/scripts/package-cyberherd.sh` creates a source-pinned review archive of
the three CyberHerd binaries, migrations and inactive examples. It has no install,
user-creation or service-start operation. CI builds and checks the CLI programs
before creating this archive; it is separate from the running Phase 1 package.

## Verification and remaining integration

Temporary-database tests use actual Unix framing and loopback HTTP with synthetic
credentials. They cover broker unsigned/signed restart recovery, changed-intent
and signature rejection, identity/rate/zero policy, wrong peers, denied arbitrary
signing, ordered reserved/no-payout export, worker lost-execute recovery, disabled
execution recovery and the supported NIP-57 path without downgrade. Non-applying
CLI fixtures check disabled startup, secret-field rejection and separate roles.
Exact results belong in a source-pinned checkpoint after CI completes.
The [runtime checkpoint](../testing/cyberherd-runtime-checkpoint-20261110.md)
records 420 passing Rust tests, seven non-skipped CLI checks and the distinct
scope of archive, core-systemd and new-broker fixtures.

The full port still needs verified payout summaries returned to membership
delivery, editable outgoing-zap text and original joining-message references,
durable note/chat/overlay outboxes and real joining publication receipts,
lifecycle notices, roster/OBS and authenticated operator tools, explicit
failed-cost/consistent restore recovery, reviewed inactive installation and live
financial activation. These examples and unit/library tests do not prove actual
cross-process installation, production provider behavior or a completed port.

Crash recovery assumes retained journals. An older payout-only database rollback
can lose an execute intent before core has a terminal report. Financial activation
must wait for independent core execution claims and explicit restore/reconciliation
tooling; never automatically resume spending from an older snapshot. This limit
does not authorize rolling back or rewriting existing financial state.
