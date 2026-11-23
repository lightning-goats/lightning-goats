# Independent payout funding — verified source checkpoint

Code source: `ce4cccaeffdf0a001b0bf7b1eaf7b636295ec16b`.
The [CI run](https://github.com/lightning-goats/lightning-goats/actions/runs/42724072762)
on that immutable source passed formatting, strict locked all-target/all-feature
Clippy, and the complete locked all-feature Rust suite: **388 passed, zero failed,
one ignored**. The separate inactive cross-host preparation example added two
passing tests and its non-applying artifact check. The ordinary suite's ignored
real-nak qualification is not included in the 388 passing tests.

This verifies the [private source qualification](../architecture/cyberherd-payout-authority.md),
[independent journal](../architecture/cyberherd-payout-journal.md) and ported
triggering-payment message metadata. The CI log explicitly passes all twelve
funding/qualification/journal tests, including the actual temporary Unix bridge
and loopback authoritative-provider fixtures. The independent-source tests include
two confirmed feedings consuming disjoint parts of one verified P2P receive,
concurrent/restarted exact admission, restore-style overconsumption with different
ledger IDs, rollback, root rebinding, held capacity after a day and clock reversal.
Private test-only constructors separately model malformed restored-core evidence.

Local formatting and strict Clippy also passed. Two local test compilations were
explicitly interrupted because of sustained swap pressure: first a default-debug
build, then the low-debug full suite after this exact source's CI was confirmed
running. Neither interrupted compilation supplies a test result. The passing CI
suite, rather than an observation timeout or a partial local log, is the full-suite
evidence. Source changes after this commit require their own checks.

The refreshed dependency audit still fails on unignored RUSTSEC-2026-0459 in
`libsqlite3-sys` 0.30.1. RSA remains unreachable from the active graph and its
existing scoped exception is unchanged. No dependency or lockfile changed.

This is not full-port completion or live payout acceptance. Every reservation
remains conservatively held. Callback/signing persistence, provider intents,
execute/reconciliation, authoritative terminal budgets, conversations, interfaces
and operations remain in the full implementation contract. No payout credential,
provider payment, live CyberHerd publication or feeder action was installed or
performed by this source work or its tests.
