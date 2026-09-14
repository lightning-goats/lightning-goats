# Phase 1 Verification Matrix — Risk-Tiered Live Pilot

Effective 2026-09-14; tracker #15. Apply
[parallel-live-pilot.md](../deployment/parallel-live-pilot.md) first.

This replaces the former all-checks-before-first-payment launch gate. Keep existing
regression suites and historical audit evidence; do not represent skipped or
unperformed cases as passed. A concrete fault blocks the affected path. The complete
checklist is not a prerequisite to the operator's small parallel live pilot.

## A. Before first pilot payment: short preflight

| Check | Required result |
| --- | --- |
| Origin/TLS/routes | `herd@feeder.lightning-goats.com` resolves to the new daemon; callback/metadata use the pilot origin; old public addresses unchanged |
| Issuance and settlement | Valid signed mainnet BOLT11 binding amount/hash/expiry; authoritative Strike completion, not a trusted webhook body alone; amount/user restrictions retained |
| Secrets and runtime | Protected receive/read-only Strike key, no spend/OpenHAB key on VPS; non-admin runtime and existing service protections |
| State and provenance | Fresh pilot durable ledger on first start, selected source/binary/config recorded; no fixture/copied production ledger; no known corruption defect in this path |
| Ability to stop | Close new pilot issuance/dispatch without deleting state or disrupting old production; preserve late settlement recovery |

No fixed payment-count/220-sat/time ceiling was adopted. The operator chooses manual
payments within configured invoice and feeding limits. Sandbox provisioning, complete
weather/site work, optional guard tooling and extra agents are not prerequisites.

## B. Before and during live feeding: inspect the actual path

| Check | Required result |
| --- | --- |
| Gateway target | Intended existing local owner, compatible UUID/completion contract; VPS reaches only the narrow gateway, no generic OpenHAB/weather/admin route |
| Parallel arbitration | Both paths obey the same local owner/interval/cap; otherwise legacy feeder dispatch alone is paused and its credit retained |
| Local safety | Override/enable, minimum interval, absolute cap and persistent duplicate controls remain effective |
| Start state | Inspect accumulated credit/feeds due/unresolved UUID before enabling; no empty-state assumption after earlier real payments |
| Delivery and debit | One actual completion for the UUID produces one debit/event; receipt alone is not completion |
| Ambiguity/capacity | Unknown result blocks fresh automatic dispatch; reached retention capacity stops admission, never causes identity deletion/reset |

A quick initial shadow payment is optional. `canary` can perform real feeds without
public Nostr; `active` additionally publishes. Do not assume a mode name identifies
the physical versus harmless gateway target. A failed physical preflight need not
prevent clearly labelled payment-only testing with new dispatch disabled.

## C. Observe through operator payments, not another synthetic launch ceremony

Record one durable credit/event per real settled receive and correct `address_user`.
Observe threshold crossing, actual feed, correlated completion, debit and remainder.
At a 1000-sat threshold with zero starting credit, 2340 sats means two confirmations,
340 remaining and no third feed. Adapt the expected result to the real starting balance.

Restart only the pilot process against its unchanged database. Confirm no extra credit
and no repeated physical command for a confirmed UUID. Inspect same-ID replay/status
without inventing a fresh request. Stop on extra feeding, contradictory money or unknown
delivery. Do not inject dangerous failures into a live actuator to fill a checklist.

Preview overlay payment/feed messages and reconnect behavior. Weather may be unavailable;
when enabled it uses actual observation age/units and remains overlay-only. In canary
mode, public Nostr is intentionally untested/off. If active mode is enabled, observe
normal payment/feed posts and exact signed-event retry without a historical flood.

Genuine Strike webhook delivery is distinct from scanner recovery. The first payment
may use recovery scanning while subscription work is pending; report that limitation.
Later verify real signed notification handling/recovery without duplicate credit. Do
not count a synthetic HMAC payload as provider delivery or a Lightning-only payment as
proof of Strike-to-Strike P2P/FX behavior.

## D. Keep the automated regression inventory

Run existing relevant suites on the selected candidate; reuse matching CI evidence.
For Rust changes:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
```

Also use the existing Security/Deployment gates. Check actual exits/logs, not a green
`tee` pipeline or an empty test selection. Ordinary-suite ignored tests are not counted
as executed; distinguish dedicated-workflow evidence. No tests or workflow assertions
are removed or weakened by this planning revision.

Preserve coverage for:

- signed invoice signature/checksum/network/amount/metadata/expiry and invalid users;
- provider errors/429/timeouts, forged/missing/reordered notifications, durable recovery,
  conflicting IDs/hashes and atomic credit/event creation;
- SQLite file/WAL/FULL startup, concurrency, duplicate/restart persistence, interrupted
  intent and ambiguity-safe UUID recovery;
- feeder override/caps/refusal, real daemon-to-gateway with harmless-owner counts,
  and ordering such as receipt before debits (do not use the known-bad #82 checker
  as accepted evidence; #91 is its correction candidate, subject to normal review);
- constrained nginx/webhook/WS ingress, authentic peer containment including forwarding,
  established flows, IPv6 and rollback; separate synthetic namespace evidence from hosts;
- overlay replay/reset/heartbeat/limits, weather units/observation age/regression,
  exact signed Nostr retries, release/installation/credential-isolation checks.

These remain engineering tools, not requirements to re-run every experiment on both
real hosts before a manually observed pilot. Do not count source review as live evidence.

## E. Follow-up and operator cutover

Additional goat-address coverage, public webhook/P2P variants, extended browser/weather
compatibility, long-retention/co-restore redesign, optional CI tooling, comprehensive
hardening and full-host rehearsals remain follow-up unless an identified defect affects
the enabled pilot. Existing local capacity and secret restrictions still apply. Never
perform destructive restore/fault experiments against paid state just to complete a row.

The operator decides when live results justify the DNS/Nostr-profile switch. Use
[production-cutover.md](../deployment/production-cutover.md), preserving pilot state,
old outstanding invoices/credit and one physical dispatcher/owner. Hub migration need
not coincide with payment cutover. Check additional addresses as they become public;
there is no requirement for six paid tests or closing every parent issue first.

## Evidence format

A short source-pinned note with starting/ending credit, receive/feed counts, restart
result, concrete failures, enabled modes and still-untested features is enough. Keep
sensitive payment/account details private. Leave unverified items open without making
each one a launch gate. Previous exhaustive matrix/history is retained in Git at
`df6be90f1b0505d183643ec55a298b382cd43b6c` and in existing audit/testing records.
