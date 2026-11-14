# Outgoing invoice preparation — verified source checkpoint

Code source: `aac30d02b5b31a9ce9fb34157405a9fff9a63942`.
The [CI run](https://github.com/lightning-goats/lightning-goats/actions/runs/42725420735)
passed formatting, strict locked all-target/all-feature Clippy and the full locked
all-feature Rust suite: **394 passed, zero failed, one ignored**. The inactive
cross-host preparation example separately passed two tests and its non-applying
artifact check. A local locked all-feature library check also passed.

The six added tests explicitly pass in the CI log:

- Ordinary fallback checks the exact metadata description hash, amount and time.
- Supported zaps preserve exact signed JSON, reject altered signer intent and
  cannot silently downgrade; semantically equivalent JSON has a different hash.
- Malformed, oversized, provider-error and private-destination cases fail without
  echoing the provider response body.
- Structured query values accommodate signed NIP-57 bytes without relaxing the
  supplied origin's HTTPS/public-address/port/credential checks.
- Concurrent/restarted ordinary preparation returns one intent, freezes one
  invoice, rejects replacement and retains historical validation after expiry.
- Supported request bytes remain frozen across reopening before callback work;
  an invoice committing to ordinary metadata is rejected for this zap.

The existing disjoint-source test additionally verifies a global payment-hash
collision between separately funded grants. The rejected second invoice leaves no
saved invoice or partial invoice-journal stage; its original callback/grant remain
available and held. No provider transfer occurs in these fixtures.

The [deployment workflow](https://github.com/lightning-goats/lightning-goats/actions/runs/42725420980)
also passed its mocked website, nginx/verifier, release build/archive smoke,
isolated installation, systemd credential sandbox and harmless home fixtures.
Those are existing deployment boundaries, not an installed CyberHerd payout worker.
The [isolated NIP-46 workflow](https://github.com/lightning-goats/lightning-goats/actions/runs/42725420652)
passed its pinned-nak/synthetic signer/relay and encrypted-credential sandbox
regressions. It does not qualify the unfinished outgoing signing broker or a new
household production client.

The [Security run](https://github.com/lightning-goats/lightning-goats/actions/runs/42725420626)
fails only the dependency audit with the known unignored RUSTSEC-2026-0459 in
`libsqlite3-sys` 0.30.1. Its RSA reverse-graph check passes. No dependency/lockfile
changed; this is not a clean security gate.

The [invoice boundary](../architecture/cyberherd-outgoing-invoices.md) is source
preparation. It neither signs through a new production broker nor quotes, executes
or reconciles an outgoing payment. Authoritative terminal budgets, notifications,
worker/service setup and the remaining full-port requirements remain unfinished.
No live CyberHerd authority or financial side effect is activated by this check.
