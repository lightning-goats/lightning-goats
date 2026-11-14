# CyberHerd payout outcome checkpoint — 2026-11-10

Verified code: `902c9a7f34e1fbfebd99b4470f9320a803321c26`, following implementation
`9c943b67cf424dd7c6b2dbcbe9a2cfbafcd2c401`. This is an isolated source checkpoint,
not a deployed payout service or completion of the twelve-part CyberHerd port.
The current upstream explanation was fetched again and still hashes to the pinned
`e56c6793ed0687e4fcac7df54dac48a375673308dc65e988e0dcd987af46e67a`.

## Verified checks

[CI 42736784793](https://github.com/lightning-goats/lightning-goats/actions/runs/42736784793),
Rust job `129567855512`, passed:

- `cargo fmt --all --check`;
- `cargo clippy --locked --all-targets --all-features -- -D warnings`;
- `cargo test --locked --all-features`: **410 passed, zero failed, one ignored**;
- the separate inactive preparation example: **two passed**, plus its non-applying
  artifact check. These two are excluded from the 410 suite total.

Completed job logs explicitly show all eight new outcome tests passing, including
actual Unix role checks, independent synthetic read/sender keys, provider
disagreement, fee savings, failed-cost retention, permanent unsubmitted closure,
execute uncertainty, partial reports, lost ACK/restart, final-write rollback,
the exact 24-hour boundary and global payment/invoice reuse rejection.

[Deployment artifacts 42736784930](https://github.com/lightning-goats/lightning-goats/actions/runs/42736784930)
passed browser, nginx, harmless home fixture, full release/archive verification,
isolated runtime identities and systemd/inactive-preparation checks.
[NIP-46 regression 42736784952](https://github.com/lightning-goats/lightning-goats/actions/runs/42736784952)
passed the existing isolated relay/client scenario with pinned real nak. It does
not qualify the unfinished outgoing signing broker or a production client.

The required [Security run 42736784826](https://github.com/lightning-goats/lightning-goats/actions/runs/42736784826),
job `129567855283`, still fails on unignored **RUSTSEC-2026-0459** in
`libsqlite3-sys 0.30.1`, with a reported fix at `>=0.39.0`. The RSA reverse-graph
guard passed. This source changes no dependency/lockfile and does not waive the
advisory or represent a clean security gate.

## What the new source establishes

The [outcome protocol](../architecture/cyberherd-payout-outcomes.md) freezes
protected payout-journal attestations and submits one bounded row at a time under
the payout UID. Core independently reads original payment IDs using read-only
authority. Original correlation and absence of execute intent remain assertions
of the trusted payout role; an optional/missing provider preimage is never
misrepresented as cryptographic correlation.

Core verifies reservation/epoch/funding, ordered hash-chain rows, terminal state
and exact costs before retaining evidence. Partial reports hold the full reserve.
Only the final count/hash/gross check releases unused allocation. Exact retries
and committed ACK reads survive lost responses and disabled execution. The payout
store holds its whole grant until authenticated final ACK. Completed costs enter
the rolling window; unresolved/failed costs remain held indefinitely. The
independent store conservatively uses local ACK acceptance for its spend window.
No carried member balance, payment credit, feeder debit or public message is
created by reconciliation.

## Remaining work

The signing broker and separate payout runtime, exact member note/chat/overlay
outboxes and joining publication receipts, lifecycle notices, roster/OBS and
authenticated operator interfaces, explicit failed-cost/consistent restore
recovery, reviewed installation and financial activation remain unfinished.
Read the [full contract](../planning/cyberherd-implementation-plan.md) before
claiming completion. This checkpoint makes no live provider, real payment,
physical feeding or installed CyberHerd acceptance claim.
