# Phase 1 Execution Plan — Parallel Live Pilot and Multi-Asset Credit

## Addition approved 2026-09-15

Read [the multi-asset architecture and implementation plan](../architecture/multi-asset-payments.md).
[#94](https://github.com/lightning-goats/lightning-goats/issues/94) adds in-house
MoneroPay alongside Strike, with native receipts -> immutable sats valuation ->
project feed-credit ledger. The child order is #95 ledger/domain, #96 quotes,
#97 separate home receive bridge, #98 intents/reconciliation, #99 overlay/payment
presentation and #100 mixed-rail acceptance/deployment scripts. This supersedes
older Strike-only/no-new-backend scope restrictions for that specific addition.

Implement the credit foundation before the final two-rail overlay client. Keep
BTC native identity valuation and the existing public LNURL path working. Do not
encode XMR as BTC millisatoshis, use either wallet balance for progress, revalue
old credit at current FX, or expose pending XMR as available feeding credit.
The lead may start disjoint domain/tests/docs work now; do not manufacture Codex
capacity or take over active shared files without coordination.

Monero stays disabled until its own path is implemented/accepted. The complete
multi-asset backlog is not a new prerequisite for the separately authorized Strike
pilot below. Preserve real paid state and existing source-review/live boundaries.
CyberHerd, automatic swapping/refunding and general frontend rewrites stay out of scope.

## Existing pilot objective

Status: operator-selected direction, 2026-09-14; not a claim of live deployment.
Tracker: #6. Canonical operating plan:
[parallel-live-pilot.md](../deployment/parallel-live-pilot.md).

Run the existing Strike-backed Lightning Goats stack on the new VPS at
**herd@feeder.lightning-goats.com** alongside the old production system. The operator
sends real payments, watches accounting/feeding/presentation, and decides when the
new system is good enough to take over established public addresses and metadata.
This is a low-traffic hobby pilot, not an enterprise launch certification exercise.

Preserve durable payment/feeding accounting, local feeder safety and the existing
Nostr/overlay architecture. Do not switch Lightning backend because sandbox access
is delayed. The approved Monero addition is a separate workstream, not a substitute.

## Superseded launch policy

The operator's pilot decision replaces the blanket HOLD and full-verification-before-
any-live-payment sequencing in older plans, handoffs and issue comments. Strike
sandbox, every open audit item, six paid goat-address tests, repeated full host
laboratories, optional pipeline guards and long-retention/restore redesign are
not prerequisites for beginning the pilot. Historical evidence stays historical.

The earlier proposed two payments of 100 sats, 220-sat total and 30-minute window
were not adopted. The operator selects manual payment amounts and duration. Keep
configured invoice limits, local feeder interval/caps and the existing account
balance policy; do not invent a new arbitrary quota or automatic payment loop.

This docs revision does not perform host changes or waive branch protections/code
correctness. A specific unresolved defect blocks its affected feature, not the
entire project. Do not self-approve a safety-critical runtime fix or force-merge an
existing changes-requested PR. The operator's planning decision needs no Codex ACK.

## Pilot work order

| Step | Deliverable | Completion evidence |
| --- | --- | --- |
| 1. Prepare the pilot | Dedicated hostname/TLS, existing daemon under a non-admin identity, receive-only Strike access, fresh pilot ledger, old production untouched | Source/binary/config pin and short preflight in the pilot runbook |
| 2. Take manual payments | Wallet resolves herd at the pilot domain; mainnet invoice reaches the new daemon | Actual settled payment and exactly one matching credit/event |
| 3. Observe real feeding | Existing local owner accepts the new gateway path; old/new dispatch cannot race outside that owner | Operator sees intended feed, correlated completion and one debit; ambiguity blocks retry |
| 4. Iterate in place | Fix observed failures, verify restart/replay, preview overlay and optionally enable normal Nostr messages | Brief source-pinned observations; no reset of paid state |
| 5. Cut over when satisfied | Operator changes established DNS/address/profile references | Existing payments/credit retained, no doubled dispatch, old VPS still recoverable |

A quick shadow payment check can precede feeding in the same session. It is not a
separate approval ceremony or a replacement for the requested live-feeder pilot.
Use `canary` for feeds without public Nostr; `active` includes public Nostr. Runtime
mode alone does not tell whether the gateway targets a harmless fixture or real owner.

## Minimal blockers versus follow-up

Before taking money: working pilot TLS/routing and authoritative Strike settlement,
protected receive-only credentials, separate durable state, a tested selected
build with no known defect corrupting this path, and a practical way to stop it.
Do not publish invoices as feeding-enabled while the physical path is still off.

Before feeding: the selected owner contract must actually support correlated
completion, persistent UUID deduplication and local override/interval/cap controls.
Confirm the narrow gateway target and arbitrate the old dispatcher. If that path
is unavailable, keep testing payments honestly as payment-only instead of inventing
completion or bypassing the owner.

Weather polish, broader browser compatibility, additional goat addresses, optional
CI tools, advanced retention/co-restore designs and unrelated hardening exercises
are follow-up unless a concrete issue affects the pilot. A deployed capacity limit
remains real: stop at exhaustion, never delete replay identities to continue.

Use existing candidate PRs rather than recreate integrations: at planning baseline
`df6be90f1b0505d183643ec55a298b382cd43b6c`, #91 contains the #82 chronology correction;
#89 contains HOME #84/#85 integration. Recheck them when selecting runtime source.
They are not declared merged/accepted by this plan. #87 and #92 are not prerequisites.

## Execution responsibilities

The lead handles repository implementation/integration work with available tools.
HOME/VPS retain their host-specific technical responsibilities when available;
Codex is not required to bring up the pilot. Do not manufacture worker capacity,
private-record access, handoffs or independent reviews.

A later instruction to bring this pilot online should be handled as one scoped
setup/test session, not serial approvals for each ordinary check. Explain any
specific necessary host/credential action and its effect. The operator pays from
their own wallet and retains the final public DNS/Nostr-metadata cutover decision.
This planning edit itself performs none of those operations.

## Verification and rollback

Use [the risk-tiered matrix](../testing/phase1-verification-matrix.md), preserving
existing tests and reusing exact-candidate CI results. A 2340-sat run at a 1000-sat
threshold should give two confirmed feeds and 340 remaining; it is a useful test,
not a mandatory purchase or arbitrary pilot budget. Apply #100's corresponding
mixed-rail checks before enabling XMR; arithmetic-only tests are not live acceptance.

Stop only the new pilot ingress/dispatcher on failure; keep its real ledger,
issued requests, pending UUIDs and signed outbox. Resolve possible physical delivery
before resuming another dispatcher. Do not reset the database or restore stale state
as an easy rollback. See [cutover/rollback](../deployment/production-cutover.md).
