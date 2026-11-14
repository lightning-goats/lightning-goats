# Authenticated CyberHerd ingress checkpoint

Source tested: `625e9364f4b6bbb6fd8d84a938423f72ac7db1ec`.
This is evidence for source components, not completion of the
[full CyberHerd port](../planning/cyberherd-implementation-plan.md).
The upstream explanation was rechecked: its file-changing commit remains
`92ecf6c52482de17d404dbf2def3c71213408140`.

## Checks actually executed

| Check | Result |
| --- | --- |
| `cargo fmt --all --check` | passed |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | passed |
| `cargo test --locked --all-features` | 325 passed, zero failed, one ignored |
| `cargo tree -i rsa --locked --all-features` | no active reverse dependency |
| `cargo audit --ignore RUSTSEC-2023-0071` | failed on existing, unignored RUSTSEC-2026-0459 |
| `git diff --check` | passed |

The one ignored ordinary-suite test is the existing real-nak namespace test;
it was not executed as part of this checkpoint. This work adds no unsafe code.
The exact timezone dependency is `chrono-tz 0.10.4`, with locked `phf 0.12.1`,
`phf_shared 0.12.1` and `siphasher 1.0.4`. Existing package versions are unchanged.
The fresh audit examined 315 packages against 1430 advisories and reported only
the [existing SQLite finding](../security/sqlite-wal-advisory-20261109.md).
No new audit exception or patched-dependency claim was introduced.

## Evidence exercised

- Real Mountain-time spring/autumn boundaries are 23/25 hours. Midnight gaps,
  skipped dates, UTC boundaries and invalid timezone/timestamp input are covered.
- Actual separate databases retain monotonic ended stream heads, old authenticated
  aliases and signed negative profile replacement across reopen and replay.
- Native project signatures, coordinate-bound host proofs and fixed-provider
  attestation establish stream authority. Arbitrary service URLs/host tags,
  expired attestation and another project's proof do not. Repeated forged
  publishers cannot repeat a metadata attempt within one observation batch.
- Current-day announcement selection prefers the stream's linked signed open
  goal; valid older stream aliases remain associated with the active coordinate.
- A real WebSocket protocol exchange over private in-memory transport rejects
  invalid signatures, unrelated events and wrong-subscription completion, and
  deduplicates valid results. Binary/capped/closed reads cannot claim complete
  backfill. Event-ID/coordinate engagement filters are separate OR alternatives.
- Social parsing accepts standard positive reactions and supported reposts,
  excludes downvotes/emoji/chat/self interactions, validates embedded signed
  targets and respects NIP-25's final target reference.
- Missing profile lookup stays pending. Unavailable target discovery or paused
  feeder state defers membership. Frozen profile/context, membership and source
  receipt survive restart and apply once; confirmed stale work receives only a
  minimal discard receipt. Matching an active coordinate does not make a raw,
  unauthenticated stream version an alias.
- Optional calendar catch-up resets once, retains lifetime/source receipt state,
  and cannot re-credit a pre-reset interaction.
- The full existing settlement, feeder, native-zap, operator-credit, outbox,
  overlay and storage regressions also passed.

The initial full run found an integration assertion that required the text
`695` in every chosen feeding template. The restored custom/legacy corpus
legitimately includes variants without a remainder sentence. The corrected test
checks the actual confirmed source event, nonempty kind-1 content, unchanged
remaining credit and single signing/outbox behavior across restart. Template
selection and financial/physical semantics were not changed to satisfy it.

## Remaining implementation

The library is described in [the ingress contract](../architecture/cyberherd-ingress.md).
It is not yet the refresh/recovery service. The authenticated core-event bridge,
daemon orchestration, exact settled-zap consumption, confirmed-feed real-fund
reservation/planning, isolated payout adapter, durable note/chat destinations,
lifecycle scheduling, interfaces and reviewed installation/activation still need
implementation and end-to-end verification. These tests do not prove public
relay completeness, household signer support for new kinds, payouts or food.
