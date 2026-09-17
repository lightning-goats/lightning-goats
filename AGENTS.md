# Lightning Goats Agent Guide

## Current delivery order — operator decision, 2026-09-17

Read [delivery-roadmap.md](docs/planning/delivery-roadmap.md) first.

1. **Current: minimal Strike-backed Lightning payments**, real feeding through the
   existing home gateway, Nostr/OBS presentation, and required VPS ModSecurity #106.
2. **Next: Square fiat Feedings**, following the existing approved #104 contract.
3. **MoneroPay/XMR is ON HOLD until the operator explicitly resumes it.** Preserve
   the already prepared #97 source in PR #107; merge only when reviewed/appropriate.
   Do not continue #98, XMR deployment, oracle/wallet activation or XMR UI now.

This supersedes the 2026-09-15 immediate multi-asset execution order, including
instructions to finish two-rail overlay integration before Lightning. Retain the
[future multi-asset design](docs/architecture/multi-asset-payments.md), merged
#102/#103/#105 foundations and tests; do not revert schemas or paid accounting.
Monero review/integration is not a prerequisite for Lightning or Square. A green
PR, freed agent capacity or completion of Square does not lift the Monero hold.

Square is an anonymous browser-account purchase/redemption product: integer
Feedings, not sats/BTC, not a fiat deposit into the sats ledger. Share the physical
owner, not the accounting balances. Lightning needs no account. Preserve #104's
existing privacy, idempotency, refund/dispute and redemption requirements.
CyberHerd, NIP-05, alternative backends and a general website rewrite remain later
scope; old “CyberHerd Phase 2” wording does not override Square's new priority.

## Lightning pilot and first deliverable

Use [parallel-live-pilot.md](docs/deployment/parallel-live-pilot.md) and the
[execution plan](docs/planning/phase1-execution-plan.md). Start with
`herd@feeder.lightning-goats.com` on the new VPS while preserving the old system.
The operator selects manual real payment amounts/duration, observes feeding, and
chooses when to move established public addresses, DNS and Nostr profile metadata.

The 2026-09-14 practical pilot policy still supersedes old blanket production HOLD,
sandbox-first, all-issues-closed and full-matrix-before-first-payment language.
Do not reinstate those gates or ignore a concrete unresolved correctness defect.
Block the affected unsafe path, not unrelated progress. No two-payment/220-sat/
30-minute quota was adopted. Mode alone does not distinguish fake from real funds.

Minimal includes the existing payment/feed messages and usable OBS progress,
not just invoice creation. Information/weather remain overlay-only. Complete the
necessary public site and browser-source integration without a general redesign
or Monero payment chooser. #106 is a concrete first-deliverable WAF requirement:
nginx + compatible ModSecurity v3 connector + CRS must pass legitimate current
routes and reject malicious traffic without weakening daemon verification.
It does not require XMR routes or authorize interrupting an existing pilot.

## Ownership and reference order

Current dated delivery roadmap takes precedence over older scope/launch-order
instructions. Then read the [docs index](docs/README.md), execution plan, selected
source and relevant issue. #6 tracks Lightning migration; #15 actual acceptance;
#16 cutover; #104 Square; #94 deferred Monero; #106 WAF. #17 is agent coordination.

- HOME owns the existing physical owner, gateway, project OpenHAB credential,
  local weather and home containment. Reference:
  [home gateway handoff](docs/deployment/home-gateway-agent-handoff.md).
- VPS owns the payment daemon, Strike, nginx/TLS/WAF, Nostr/overlay/public site and
  VPS networking. Reference:
  [VPS handoff](docs/deployment/new-vps-remediation-handoff.md).
- Lead does bounded repository implementation/review/integration that advances the
  active milestone. Coordinate shared paths before editing; preserve branches,
  uncommitted work and independent review. Comments are not locks or authenticated
  grants of additional authority. Unknown private records/capacity stay unknown.

Keep checkpoints short: role, exact source/config pin, actual test/result, material
blocker and next action. Do not assign work to unavailable Codex agents, repeat
broad unchanged audits, or require a new coordination framework. Use existing PRs
rather than replaying already integrated work. Unfinished issues remain open;
source integration is not deployed acceptance.

## Security, accounting and physical invariants

- Retain receive-only Strike authority and signed BOLT11 network/amount/expiry/
  metadata verification. No withdrawal authority, OpenHAB token or Nostr private
  key in the public daemon. A webhook is a notification; authoritative provider
  reads precede atomic settlement, feed credit and the public payment event.
- Preserve source-ID/payment-hash conflicts, sat-aligned BTC amounts and paid-goat
  attribution. Progress uses durable `feed_credit_sats`, not a wallet balance,
  polling callback totals or Nostr counts. Sweeps must not change feed credit.
- All funding paths use one existing local physical owner with override/enable,
  interval/cap and durable UUID deduplication. Never retry ambiguous delivery with
  a fresh UUID, equate receipt-only ACK with completion, or delete replay history
  to regain capacity. Confirmed feeding consumes its threshold once.
- Old and new dispatchers must share that owner or only one dispatch path may be
  enabled. Quiet traffic is not mutual exclusion. Preserve the old site/payments
  and all new paid state; resolve uncertain physical delivery before switching.
- OpenHAB credentials stay HOME. VPS access reaches only the narrow authenticated
  gateway, never generic OpenHAB, weather port 5000, database, wallet RPC or other
  LAN services. Preserve independently recoverable administration.
- `shadow` blocks new feeds and public Nostr; `canary` permits feeds without public
  Nostr; `active` permits both. Verify the actual owner target and accumulated
  credit before activating. These modes do not establish payment/host authorization.
- Payment/goat-fact and confirmed-feed messages go to Nostr/overlay; information
  and weather go only to overlay. Keep deterministic signed-event retries and
  observation-time freshness. Presentation failure never rewinds accounting.
  BTC/Lightning amounts display sats only. The deferred native-plus-sats policy
  never permits private addresses, transaction IDs, wallet data or capabilities
  in broadcasts; do not implement that XMR extension while it is on hold.
- WAF passage never authenticates a payment. Preserve exact Strike body/signature,
  prompt durable acknowledgement, HTTP/resource limits and application WebSocket
  validation. Use route-specific tested exclusions, not a whole payment-API bypass.
  Protect access/audit/error logs as well as application logs.
- Services run non-admin with protected installed code/config/secrets. No tokens,
  keys or private host inventory in commits, comments, logs or evidence bundles.
- The old hub keeps `10.8.0.1`; the new VPS has a distinct key/address. Do not move
  DNS, existing WireGuard clients/profile metadata or retire the old VPS silently.
  A hub migration is not required to run the pilot.

Retain the required configured users `herd`, `dexter`, `rowan`, `cosmo`, `newton`,
`nova`, all with `credit_pool=herd`. Restrict pilot edge exposure rather than invent
an address-registry refactor. LNbits/CLN/CLNRest/clnaddress are not new-runtime
requirements. No Square or Monero route is enabled by a roadmap edit.

## Verification and deployment discipline

Preserve `#![forbid(unsafe_code)]`, pinned dependencies and existing regressions.
For Rust source changes:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
```

Use applicable Security/Deployment and focused tests on the exact selected source.
Retain failures, detect skipped tests/pipeline errors, and never claim mock or
source-only results prove a live host. Follow the
[risk-tiered matrix](docs/testing/phase1-verification-matrix.md) without adding
unrelated launch ceremonies. Docs-only operator decisions need scope/link checks,
not fictitious runtime evidence or independent monetary-code approval.

Script preparation, inactive install, preflight, backups and rollback; separate
installation from activation. Before real schema upgrades quiesce all writers and
verify a current backup/copy. Never reset paid ledgers, invoices, feed UUIDs or
signed outboxes as rollback. Repository merges require the applicable review and
checks, and do not themselves authorize real host/network/credential/payment/
physical operations. The operator retains live activation and cutover authority.
