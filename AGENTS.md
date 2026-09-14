# Lightning Goats Agent Guide

## Current priority: parallel live pilot (operator decision, 2026-09-14)

Read [the parallel live pilot plan](docs/deployment/parallel-live-pilot.md) first.
The operator wants `herd@feeder.lightning-goats.com` on the new VPS, real manual
payments and observed feeding, with the old production system kept available.
Later production DNS and Nostr profile metadata changes are the operator's decision.

The pilot plan supersedes blanket **production HOLD**, sandbox-first, all-issues-
closed and full-matrix-before-first-payment language in older handoffs, audit
addenda and issue comments. Those remain technical references and historical
evidence, not an automatic veto on this pilot. Do not mark unperformed tests as
passed or dismiss a concrete unresolved correctness defect. Block only the affected
unsafe feature, not unrelated progress. This planning change does not execute
host changes, payments or feeding, nor approve unreviewed runtime code.

## Work that advances the goal

Use the existing Strike implementation. Prepare the pilot hostname/TLS, receive-only
credentials, isolated durable state and existing gateway path; then observe real
payments and feeds with the operator. No new backend, website rewrite, mandatory
Codex installation, coordination framework or CI-parser expansion is needed.
Sandbox access and completion of every historical audit exercise are not prerequisites.
There is no adopted two-payment/220-sat/30-minute limit: the operator chooses manual
test amounts and duration, within the configured application and local feeding limits.

Keep checkpoints short: source/config pin, actual result, material blocker and next
action. A known source defect needs a focused fix/review; do not create another
integration PR for a candidate that already exists. Respect repository protections
and existing code-review findings; do not invent independent approval. Codex capacity
is currently operator-reported exhausted. Do not assign work to unavailable agents
or make a worker reply a prerequisite for a docs-only operator decision.

## Ownership and reference order

- HOME owns the in-house gateway, OpenHAB credential, existing physical owner,
  local weather and home-side containment. Its technical reference is
  [home-gateway-agent-handoff.md](docs/deployment/home-gateway-agent-handoff.md).
- VPS owns the payment daemon, Strike, nginx/TLS, Nostr/overlay and VPS networking.
  Its technical reference is
  [new-vps-remediation-handoff.md](docs/deployment/new-vps-remediation-handoff.md).
- Coordinate active/shared runtime paths before editing them. GitHub comments are
  an audit trail, not locks. Preserve existing private task records; unknown task
  generations/digests and capacity stay unknown. No new protocol/store is implied.

The dated pilot plan takes precedence over launch-order restrictions in both
handoffs. Then read [docs/README.md](docs/README.md), the
[execution plan](docs/planning/phase1-execution-plan.md), the relevant source and
issue. #6 tracks migration; #15 tracks observed acceptance; #16 tracks later cutover.
#17 and #21 remain gateway/weather technical references. Open issues are not all
pilot blockers. Do not replay the already integrated #31-#55 stack.

## Keep the practical safety and accounting invariants

- Strike is the selected backend. The public daemon gets receive/read authority,
  never spend/withdraw authority, an OpenHAB token or a Nostr private key.
- OpenHAB credentials stay on the home gateway. VPS application traffic reaches
  only the narrow gateway, not generic OpenHAB, weather port 5000 or unrelated LAN
  services. Preserve existing administrative recovery access.
- Verify signed BOLT11 network, amount, expiry and exact LNURL metadata hash.
  Treat webhooks as notifications; reconcile authoritative provider state before
  atomic settlement, feed credit and `payment_received` creation.
- Preserve source ID/payment-hash idempotency, sat-aligned accounting and the paid
  `address_user`. Unknown users/invalid amounts fail before provider contact;
  retain public rate/body/method limits.
- One existing physical owner must arbitrate both payment paths. Keep local
  override/enable, interval/cap and durable UUID duplicate controls. If legacy
  dispatch bypasses that owner, pause only legacy feeder dispatch while the pilot
  feeds; keep the old site/payment state intact. Quiet traffic is not mutual exclusion.
- Never retry an ambiguous feed with a fresh UUID, reset paid state, delete old
  replay identities to regain capacity, or treat receipt-only ACK as completion.
  Preserve all issued requests, real credits, pending UUIDs and signed events.
- `shadow` blocks new feeds and public Nostr; `canary` permits feeds but not public
  Nostr; `active` permits both. Modes do not distinguish fake money from mainnet.
  Check the actual gateway target and accumulated credit before enabling feeding.
- Payment/confirmed-feed messages may reach Nostr and overlay. Info/weather are
  overlay-only. Do not replace observation time with poll time or generate a new
  signed Nostr event for a publication retry. Presentation failure never rewinds money.
- Runtime services are non-admin; installed code/config and secrets are protected.
  Never put secrets in commits, comments, logs, prompts or evidence bundles.
- The old hub keeps `10.8.0.1`; the new VPS uses its own distinct key/address.
  Do not silently move production DNS, existing WireGuard clients or public profile
  metadata, or retire the old VPS. A hub migration is not required for the pilot.

The current config validator requires `herd`, `dexter`, `rowan`, `cosmo`, `newton`,
`nova`, all using `credit_pool=herd`. Keep that registry; initially expose only herd
at the pilot edge instead of adding a registry refactor. CyberHerd business logic
and reintroducing LNbits/CLN/CLNRest/clnaddress into the new runtime remain out of scope.

## Verification and engineering

Preserve `#![forbid(unsafe_code)]`, pinned dependencies and existing regressions.
For Rust changes run:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
```

Use applicable Security/Deployment and focused tests on the selected source; reuse
valid exact-candidate evidence rather than rerunning unrelated laboratories at each
checkpoint. Do not weaken failing assertions or conceal skipped tests/pipeline
failures. Distinguish implemented, tested, merged, installed and observed live.
For docs-only changes verify scope, links, runtime-mode/route accuracy and internal
consistency; do not claim host testing. See the
[risk-tiered verification matrix](docs/testing/phase1-verification-matrix.md).
