# Inclusive CyberHerd fees — partial integration, 2026-11-11

The operator selected actual Strike fees deducted from each member's gross
share, without a separate fixed fee cap. Spending limits include fees: 100 sats
per confirmed feeding and 1,000 sats per rolling day, with at most 100 sats
received per payment and 100 new signing requests per hour. These inputs are
complete; financial activation is not implied.

The upstream explanation was fetched again and its SHA256 remains
`e56c6793ed0687e4fcac7df54dac48a375673308dc65e988e0dcd987af46e67a`.
The [full twelve-part plan](../planning/cyberherd-implementation-plan.md) remains
active and incomplete. The user-selected confirmed-feeding accounting and
three-user capacity continue to override the legacy explanation.

## Source behavior

- Checked whole-sat arithmetic reduces an oversized invoice by the quoted fee.
  A subsequent lower fee never increases a previously requested amount. Fees
  consuming the share or violating recipient/frozen planning minima make it
  unaffordable; unused allocation remains with operations.
- Strike quote parsing produces opaque, non-executable fee observations. Original
  provider scope, quote/request identity, BOLT11 amount/hash/expiry, fee and total
  remain bound. An oversized observation cannot become an executable quote.
- Migrations 0009–0010 add append-only observations and separate pre-execution
  invoice/callback/quote identities. Every reduction follows the preceding
  oversized quote, within eight rounds, without changing the original grant or
  the existing single executable invoice per member.
- The persistence adapter freezes exact signed zap bytes, validates each invoice,
  restores the same request after restart, and rejects conflicting retries.
  Historical reads reverify the BOLT11 against the original frozen request.
  Discovery loading is separate from executable quote loading. Even a fitting
  probe cannot enter the execution chain without a later explicit promotion.
- New reward plans retain the selected custom zap template and member name, so
  reduced invoices render their exact net amount. Historical plans omit the
  optional field and retain their serialization/hash. Legacy rendered wording
  cannot describe a changed amount; template opt-out and later edits remain
  respected.

The API reference is [Strike's Lightning quote endpoint](https://docs.strike.me/api/create-lightning-payment-quote/).
Its amount override/inclusive policy is documented for zero-amount invoices;
fixed-amount LNURL/NIP-57 invoices require their own exact adjusted request.

## Verification

Source `f46fe76469056742811c5495eebd2b8f31576dea` passed all ten workflows,
including [Rust](https://github.com/lightning-goats/lightning-goats/actions/runs/43053285045),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/43053285065)
and [Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/43053285108).
Seven local SQLite tests exercise the actual complete payout migration sequence,
strict reduction, unaffordable fees, immutable history, backup, and refusal to
execute probe quotes. They establish schema constraints, not live authority.

The typed persistence adapter at `671795dd109fabadc9cf0f934dec93b1c925cbe0`
passed [Rust CI](https://github.com/lightning-goats/lightning-goats/actions/runs/43054767912):
534 ordinary native tests, five explicit ignores and five separately executed
mandatory root fixtures. The two discovery persistence/restart tests passed.
Its deployment nginx job failed before tests because no configured Ubuntu mirror
was reachable; that is retained as a failed result. The later source's nginx
job passed. Frozen planning minima were subsequently preserved in source
`5a3947b7c9bcca4511744a0526c2b074e977ebd2`, whose
[Rust CI](https://github.com/lightning-goats/lightning-goats/actions/runs/43055112612)
also passed.

Amount-aware frozen wording initially failed compilation because of an incorrect
module path. The corrected source `bea3baf55296343b45600f7908ac54f511d8c521`
passed all ten workflows, including [Rust](https://github.com/lightning-goats/lightning-goats/actions/runs/43056261673),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/43056261662)
and [Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/43056262037)
qualification. This covers its formatting, Clippy, native tests, security,
website/nginx, package and installed systemd fixtures. It does not qualify later
source changes.

## Integrated worker and candidate qualification

Source `9c4411b354095d0400e5d970c95de9bb7db23fa5` passed all ten workflows,
including [Rust](https://github.com/lightning-goats/lightning-goats/actions/runs/43060846490),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/43060846515)
and [Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/43060846488).
Rust ran 538 ordinary native tests, with five explicit ignores covered by five
mandatory separately executed root fixtures. The ordinary and NIP-57 HTTP worker
fixture passed invoice reduction, restart, unchanged final identities and recovery
after a lost execute response. The promotion fixture passed rollback of a partial
execution-chain copy. Planning and verified-result fixtures passed explicit mode,
legacy serialization/hash preservation and actual-net/minimum checks.

The initial worker fixture failed because its back-to-back calls exhausted the
core IPC budget. The corrected fixture follows its polling interval and verifies
that the simulated failure followed a real mock-provider submission. Core limits
were preserved. The failed result remains available in
[the earlier run](https://github.com/lightning-goats/lightning-goats/actions/runs/43059527805).

Protected candidate preparation and its independent native checker now require
matching modes and zero fixed fee floor/cap for inclusive mode. The disposable
installed rehearsal uses the selected 10%/100/1,000/100/100 policy. Seven local
schema tests passed, and all ten focused protected preparation tests passed with
actual disposable root-owned files and mocked native/service operations.

The subsequent duplicate-identity check at
`c7487ff0d6a50a9505a12f59f7cca5b8a725a4fa` also passed
[Rust CI](https://github.com/lightning-goats/lightning-goats/actions/runs/43061538713)
with 538 ordinary native tests and the mandatory root fixtures, plus
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/43061538749).
The restart fixture rejects both a changed fee-observation ID after a saved
duplicate and a changed duplicate ID after a saved observation.
Its [Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/43061538704)
also passed, including package, website, nginx and installed systemd acceptance.
All ten workflows are successful for this source. This qualifies the disposable
source/installation fixtures, not live provider behavior or production activation.

## Remaining before use

Independent artifact/source verification, current actual-host qualification,
reviewed schema/configuration maintenance, real provider/recipient acceptance and
external recovery adoption remain separate requirements. No new schema or fee
code from this checkpoint is installed on the live payout journal. Outgoing roles
remain disabled. Preparation and passing source/disposable fixtures do not
authorize live transfers or public CyberHerd rollout. The full twelve-part port
remains incomplete.
