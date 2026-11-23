# Outgoing provider journal — verified source checkpoint

Code source: `85c67cafd3afdbf48286cbbce13b8b95736ccc1a`.
The [CI run](https://github.com/lightning-goats/lightning-goats/actions/runs/42730097188)
passed formatting, strict locked all-target/all-feature Clippy, and the complete
locked all-feature Rust suite: **402 passed, zero failed, one ignored**. The
inactive-preparation example separately passed two tests and its non-applying
check. Local formatting, locked all-feature library checking and strict Clippy
also passed; the local Clippy handle completed successfully.

The eight new provider/workflow tests pass explicitly in the immutable-source log:

- A lost usable execute result leaves a committed unknown execution. Reopening
  the store retries the same quote, recovers the original payment ID and reads its
  eventual completed state. Two PATCH requests create one simulated payment.
- A lost usable quote result recovers only an ID; that cannot execute without
  verified costs. Explicit retirement is allowed before execution intent, and
  forbidden afterward, including across reopening.
- Disabled independent policy, stale/disabled core authority, reduced fee limits
  and missing public-receipt authority refuse execution. Expired permits cause no
  HTTP call. The actual core-role fixture now also covers canary mode's Nostr flag.
- Wrong currency, non-sat-aligned/negative/wrong amount, excess/inconsistent fee,
  expired quote, conversion or credential/account scope cannot authorize execution.
- Internal payment reads without Lightning data/preimage are accepted only with
  exact bound payment identity and financial terms. An invalid supplied preimage
  is rejected. A contradictory terminal state persists a conflict across restart.
- Optional quote expiry and fee fields retain their documented meaning. A local
  thirty-second validity cap does not invent an absent provider expiry.
- Structured error recovery checks actual HTTP status, exact code/property and
  original UUID, never provider message text.
- A provided preimage must match the invoice hash; on-chain details cannot replace
  the expected method.

Workflow fixtures use real loopback HTTP and Unix framing with temporary core and
payout databases. Lost usable results are simulated by errors after the mock
provider records the operation. They are not production transfers, physical
feedings, a power-loss test or qualification of the future worker process. The
existing dedicated real-nak test remains ignored in the ordinary Rust suite.

The [deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42730097081)
and [isolated NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42730097140)
workflows completed successfully. Their existing isolated artifacts/synthetic
signer coverage does not prove installation of a CyberHerd payout daemon or broker.
The [Security run](https://github.com/lightning-goats/lightning-goats/actions/runs/42730097091)
still fails only the known unignored RUSTSEC-2026-0459 dependency advisory in
`libsqlite3-sys` 0.30.1; its scoped RSA reverse-graph assertion passes.
Dependencies and lockfile are unchanged.

The [provider execution implementation](../architecture/cyberherd-provider-execution.md)
still holds all reservations. Core-authoritative terminal verification/release,
actual rolling spending, signing broker, worker/service setup, treat/thread delivery
and the remaining full-port interfaces remain unfinished. No live authority or
financial side effect is enabled by this source checkpoint.
