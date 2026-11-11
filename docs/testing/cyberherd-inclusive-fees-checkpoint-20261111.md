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

## Remaining before use

The next source adds the explicit inclusive policy mode, worker scheduling,
atomic final-invoice promotion, current-limit rechecks and actual-net verified
result accounting. New ordinary/NIP-57 HTTP tests cover invoice reduction,
restart, unchanged final identities and recovery after a lost execute response;
a transaction-failure fixture checks rollback of a partially copied execution
chain. These additions require full CI qualification. Root financial-candidate
configuration and independent deployment checks now accept the explicit mode,
requiring zero independent fixed fee cap and matching membership/payout modes.
The disposable installed rehearsal selects the operator's inclusive settings.
Its updated native checker and full worker execution still require CI qualification.
Seven local schema tests pass; ten focused protected financial-preparation tests
pass with actual disposable root-owned files and mocked native/service operations. Provider/recipient
acceptance and external recovery adoption remain separate. No new schema or fee
code from this checkpoint is installed on the live payout journal. Outgoing roles
remain disabled. Do not substitute the old fixed fee allowance for this policy or
activate the partial adapter.
