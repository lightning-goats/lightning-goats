# CyberHerd implementation — operator decision, 2026-11-09

This is the current implementation work, following the restored live Strike,
feeding, Nostr and OBS path. It supersedes the older roadmap's deferral of
CyberHerd; it does not resume Monero or change the paid ledger, physical owner,
domain/WireGuard setup or existing receive-only daemon credential.

## Authorities and explicit overrides

The behavioral reference is
[the current explanation](https://github.com/lightning-goats/cyberherd_extension/blob/main/cyberherd_explanation.md),
pinned at file-changing commit `92ecf6c52482de17d404dbf2def3c71213408140`.
The raw UTF-8 file has SHA256
`e56c6793ed0687e4fcac7df54dac48a375673308dc65e988e0dcd987af46e67a`.
Implementation and tests in that extension are behavioral references; LNbits
framework code and credentials are not new runtime dependencies. Local ownership
remains defined by [the Phase 2 boundary](../architecture/cyberherd-phase2-boundary.md).

Operator instructions take precedence where the reference differs:

- **Membership never requires NIP-05.** Signed-event identity remains mandatory;
  a payout address must still be valid before paying. Free social entry retains
  its Lightning-address requirement. Zap displacement provides the incentive.
- **Rewards follow confirmed feedings.** There is one reward opportunity per
  confirmed feeder UUID. Reaching a balance threshold, receipt/command acceptance,
  failed/paused requests or uncertain outcomes do not authorize distribution.
  Do not port the legacy three-feed/four-distribution batch rule or introduce
  debits for food that was not delivered. Existing feed credit, queue and local
  caps remain under the current daemon/gateway/owner authority.
- Preserve the existing 10-sat payment-announcement floor; smaller legitimate
  settled zaps can update an active member's score without a payment announcement.
  Social notifications are separate from payment announcements.
- Weather/information remain overlay-only. Existing scrolling messages have no
  paused prefix; the prominent paused/queued banner remains. CyberHerd adds only
  its specifically defined public event categories.

The uploaded original OBS artwork is already deployed and must be retained.

The operator-supplied legacy template export is imported as
`templates/cyberherd-legacy.json`: 50 categories, 653 variants. The original export
SHA256 is `aeed7d7c1b6c08e577ae8635f4b55fab2b0bec6efb7f4158741f0f687be51361`.
Only wording is imported; the two structured thank-you entries contribute their
`content`, without importing per-template relay routing. Rendering normalizes the
single known `{new_amount:g}` modifier to a simple placeholder with an already
formatted numeric value. No Python expression, attribute or indexed access runs.
The behavioral messaging reference is
[`cyberherd_messaging`](https://github.com/lightning-goats/cyberherd_messaging).
Payment/feed/information pools now use this export in the feature source. The
operator explicitly retains optional goat naming: selected variants containing
`{goat_name}` name a goat; generic variants remain generic, without an appended
name. Selection varies across events and stays stable on retries. Payment notices
still require 10 sats; information/weather remain overlay-only. This source change
does not claim installation on the live service.

## Runtime and money boundaries

Implement a separate `cyberherdd` trust domain for identity/target ingestion,
durable membership/interaction state, reward planning and presentation. Keep
`lightning-goatsd` receive-only and independent of membership availability.
Outbound Strike authority belongs only to a separately configured
`cyberherd-payoutd`, without public ingress, an OpenHAB token or feeder commands.
All components use root-owned installed code/config and non-admin identities.
Nostr signing remains with the household nak/NIP-46 bunker.

Core settlement and feeder records are authoritative. A social action or relay
receipt cannot mint feed credit, forge a settlement, or reserve physical work.
Any local event/API bridge must expose bounded authenticated roles, prohibit
forged payment/feed events, and preserve source identities and replay protection.
CyberHerd must not gain writable access to the core financial store or a generic
trusted-network proxy. New source stays isolated from the running services until
its actual installation/activation is selected.

Payout plans consume actual reserved funds, not an assumed wallet balance or
synthetic operator credit. Operator-funded reward reserves, if supported, require
their own explicit auditable funding authority; do not fabricate provider receipts.
Remainders and fees cannot overspend the configured member allocation. The goat
portion stays assigned to operations; no implicit balance/sweep-policy change.

## Complete behavior to implement and verify

The list is the full port contract, not a completion claim for an initial slice.

1. **Membership and exact arithmetic:** configurable joining minimum/capacity/member
   pool; no NIP-05 requirement; validated identities/addresses; active-member
   positive zap increases; zap-first displacement, strict exceed-lowest for paid
   members, repost replacement of zero-zap members, and no reaction headbutts.
   Eviction resets current zap score and preserves cycle social awards/lifetime
   history. Bans require explicit unban. Zero configured allocations stay zero.
2. **Points and shares:** weights `10*zap_sats + max(1,minimum)*(3*repost + like)`;
   first social types once per owner/pubkey/cycle across all targets, pre-join
   evidence promoted once after successful admission, no extra reconciliation
   message, replay-safe rejection, and no free re-entry using an already awarded
   type. Shares use exact six-decimal micro-percentages with stable largest
   remainders, matching legacy `services/shares.py`; only eligible active payout
   addresses divide the member pool. No extra per-slot reservation.
3. **Durable cycles:** transactional membership/score/award/event updates;
   owner isolation; immutable source identities; distinct settled invoices count
   separately even if their signed zap request is reused. Daily local-time reset
   uses operator-selected `America/Denver` (Mountain time, including DST), is
   optional and catches up before work after downtime. The same calendar decides
   which announcement is today's note. Manual
   reset is atomic, starts a new cycle and emits no public daily-reset notice.
   Neither reset erases the core paid ledger or enables pre-reset replay.
   Manual edits/removals/activations are auditable and idempotent.
4. **Authenticated targets and profiles:** discover one current-day tagged project
   announcement, one latest live 30311 stream, and its signed linked 9041 goal;
   prefer the announcement associated with the stream goal. Preserve older
   versions of the same stream as aliases. Project signatures, NIP-53 host proof
   or the fixed supported provider metadata establish host authority; an arbitrary
   host tag/URL is insufficient. Newer ended/stale updates cannot be reverted by
   older live events. Live streams can cross local midnight while fresh.
5. **Nostr ingestion and pause recovery:** bounded reconnect/backfill and separate
   OR queries for `e`/`a` references; valid kinds 6/7 and stream kind-16 reposts;
   positive reactions only; chat text earns no points. While override is ON,
   unknown or unavailable, defer membership changes and membership-thread posts
   without claiming work as applied. On resumption refresh targets first, then
   replay only still-active targets/aliases. Stale deferred work receives a
   minimal durable discard receipt; unavailable discovery must not discard it.
   Active ordinary member zaps retain their broader project-content eligibility.
6. **Native incoming NIP-57:** optional authenticated zap request on LNURL callback,
   invoice description hash bound to its exact saved bytes, source payee/amount/
   note or stream identity validation before provider contact, immutable context,
   authoritative Strike settlement and exact-once membership consumption. Relay
   receipts alone never credit money or points. Financial validity does not
   depend on profile fetch, membership admission, pause state or payout health.
   Publish any supported incoming receipt from a dedicated durable payment-receipt
   intent; retain ordinary Lightning compatibility and existing ingress limits.
7. **Confirmed-feeding reward plans:** stable owner/feed UUID deduplication, frozen
   eligible roster/points/shares/fees and real funding evidence; no distribution
   for merely funded credit or ambiguous feeding. Payment processing preceding a
   confirmation must be classified before freezing its reward plan. Whole-sat
   rounding and fee/recipient minima skip unaffordable shares with no carry-forward
   member balance; unused allocation remains for goats. Shared/ambiguous recipient
   addresses and operator transfers do not create false member treat notices.
8. **Isolated outgoing payments:** bounded credential and spending policy; persisted
   intents, provider correlation/idempotency and reconciliation before retry;
   unknown submission outcomes never get a fresh payment. Recipient LNURL fetching
   is SSRF/redirect/body/time bounded. When NIP-57 is supported, freeze/sign 9734
   before invoice request, use its exact description hash, exact whole-sat amount,
   expiry and valid mainnet signature. Never publish 9734 directly to relays or
   silently downgrade a supported recipient on failure. Record unsupported-recipient
   ordinary-Lightning fallback. Only authoritative settled payout state authorizes
   a treat notice; recipient 9735 receipt publication is separate evidence.
9. **Messages and exact delivery:** editable safe templates preserving custom edits
   and intentional deletions; frozen per-interaction contribution increments,
   cumulative score/percentages and capacity. Changed-allocation compact roster
   footer is part of the same message, with names and no extra mentions. Duplicate
   and unchanged allocations do not emit another footer. Payout messages use their
   own frozen payout breakdown. Interaction identity is owner/member/source-scoped
   across live/recovery/type aliases and `msg:` aliases; never re-sign a retry.
   Optional no-treat repost suggestions appear only without an already-earned
   repost award and at most once per owner/member/membership cycle.
10. **Conversation destinations:** regular per-user note thread plus active-stream
    kind-1311 chat per interaction, one WebSocket copy with human display names.
    Treats retain the original associated note/member thread; wait for real joining
    receipts rather than inventing anchors, independently of money movement.
    Feed announcements preserve historical templates and one active-stream copy;
    no command-acceptance/batch/uncertainty diagnostics become public food claims.
    Stream replies retain stable `a` root and direct joining-message `e` reply;
    separate branches share one score and survive stream changes/restarts.
11. **Optional lifecycle messages:** authenticated live-stream invitation at most
    once per two hours per stream, confirmed unpaused only, with template opt-out.
    Durable ordered pause/resume notices once per destination, quiet initial OFF,
    initial ON announcement, and unknown readings never announcing resumption.
    Evening wording uses sanitized gateway solar context or conservative local
    time fallback, never operates the feeder/stream, never repeats due to changing
    daylight, and can use an authenticated stream ended within 15 minutes.
12. **Interfaces and operations:** readonly roster/targets/scores API and live OBS
    cards; browser Nostr identity through the existing extension/bunker flow,
    never raw nsec; authenticated operator settings/templates/member/reset tools;
    separate daemon/payout units and credentials, reviewed inactive installation,
    restart/restore/idempotency evidence and explicit live financial activation.
    Daily CyberHerd announcement publication remains a separate process; missing
    today's valid note does not cause the membership worker to fabricate one.

## Work sequence and evidence

- Domain/state: exact points, headbutts, pre-join evidence, transactional replay,
  pause/defer/discard and reset behavior, with source-reference regression fixtures.
- Protocol/bridge: signatures/target authentication, incoming zap-bound invoices,
  core event export and capability-limited CyberHerd event import, cursor recovery.
- Isolated rewards: confirmed-feed funding reservation, frozen planning, guarded
  outgoing invoice/provider adapters and ambiguity-safe reconciliation.
- Presentation: persisted custom templates, frozen note/chat/receipt outboxes,
  human-name overlay roster, operator/client interfaces and lifecycle messages.
- Operations: all required Rust checks, security gate with known findings reported,
  relevant integration/browser/sandbox tests, clean source-pinned artifacts and
  truthful inactive/live acceptance. Never claim a passing plan or domain test
  proves external transfers or the full port is finished.

Track evidence for every numbered requirement and explicit operator override.
The active thread goal stays open until the requested implementation is complete
and verified; partial engines/models or a plan alone do not satisfy it.

## Current source checkpoint — incomplete port

The isolated `feat/cyberherd-20261109` source currently contains:

- Exact points/shares, admission/headbutts, social awards and pre-join evidence,
  replay protection, bans and Mountain-time cycle resets in a separate store.
- Imported safe editable templates with deletion tombstones and optional goat
  naming. Payment announcements retain the 10-sat floor; information/weather
  remain overlay-only.
- Optional [native incoming zaps and receipts](../architecture/native-nip57.md),
  [authenticated targets/profiles](../architecture/cyberherd-ingress.md), and
  [ordered intake](../architecture/cyberherd-worker.md) through the bounded
  [private core bridge](../architecture/cyberherd-core-bridge.md). Real settlement
  and feeder evidence remain independent of membership or relay availability.
- [Confirmed-feeding planning](../architecture/cyberherd-reward-planning.md) and
  [core funding reservations](../architecture/cyberherd-reward-funding.md) with
  frozen eligible payees, fees, source evidence and allocations. Virtual credit
  cannot authorize payouts, and no member reward balance carries forward.
- An independent [payout journal](../architecture/cyberherd-payout-journal.md),
  [source qualification](../architecture/cyberherd-payout-authority.md),
  [invoice validation](../architecture/cyberherd-outgoing-invoices.md),
  [provider execution/recovery](../architecture/cyberherd-provider-execution.md),
  [core outcome verification](../architecture/cyberherd-payout-outcomes.md) and
  [separate worker/signing broker](../architecture/cyberherd-payout-runtime.md).
  Unknown execution recovers only its original quote/payment. Failed or uncertain
  costs remain held. The receive core never loads the spend key.
- [Core execution claims](../architecture/cyberherd-execution-claims.md) before
  provider submission, including actual payout-only snapshot rollback fixtures.
  Missing original history stays held; matching history recovers the same payment
  even after quote expiry. The [425-test checkpoint](../testing/cyberherd-execution-claims-checkpoint-20261110.md)
  records this source verification and its wider restore limits.
- [Final verified result handoff](../architecture/cyberherd-verified-results.md)
  to membership, with atomic exact receipts and frozen per-member payout
  breakdowns. The [429-test checkpoint](../testing/cyberherd-results-checkpoint-20261110.md)
  records the verified accounting-to-presentation boundary.
- Subsequent [member conversation/delivery source](../architecture/cyberherd-member-delivery.md):
  admission and feeding-frozen branches, editable outgoing zap text and real
  joining-reference reads, plus optional exact note/chat/overlay outboxes.
  Replies wait for actual joining publication receipts. Verified human-name
  overlay notices remain independent during overlay-only staging. Public signing
  and publishing require confirmed unpaused/public status. The
  [439-test checkpoint](../testing/cyberherd-member-delivery-checkpoint-20261110.md)
  records source verification, not production activation.

The payment announcement deduplication and triggering-payment amount corrections
are retained without changing the original legacy export or custom templates.
The uploaded OBS artwork, base feed-credit accounting and feeder owner remain
unchanged. No new CyberHerd service or financial operation is installed or enabled
by these source checkpoints. Keep the full PR draft while required work remains.

Subsequent optional [lifecycle source](../architecture/cyberherd-lifecycle.md)
adds an actual-override observer, ordered per-relay/destination notices, bounded
live-stream invitations and sanitized gateway solar context. The
[449-test checkpoint](../testing/cyberherd-lifecycle-checkpoint-20261110.md)
records verified source and inactive artifacts. It remains disabled by default
and does not establish production installation or public relay/bunker qualification.

Subsequent [confirmed-feeding announcement source](../architecture/cyberherd-feeding-announcements.md)
adds a default-off core presentation handoff, frozen herd summary, one note/chat/
overlay per triggering payment and a correlated typed overlay import. The
[458-test checkpoint](../testing/cyberherd-feeding-announcements-checkpoint-20261110.md)
records verified source and inactive artifacts. It has not been installed or activated.

Subsequent [read-only roster/OBS source](../architecture/cyberherd-public-roster.md)
adds a disabled loopback cache API, exact ranked scores/shares, paginated static
view and typed snapshot-safe OBS cards. Optional no-treat repost hints now use
an atomic once-per-owner/member/cycle claim and preserve template opt-out. These
additions passed the [466-test source checkpoint](../testing/cyberherd-roster-checkpoint-20261110.md),
including browser/CLI/inactive archive checks; no installation or activation
is implied by their presence.

Subsequent [operator-interface source](../architecture/cyberherd-operator-interface.md)
adds optional exact-request NIP-98 sign-in, self reads, root-pinned operator
settings/template/member/reset commands and existing-signer browser controls.
Atomic revisions, retained command receipts and core ordering fences protect
manual changes and frozen feeding snapshots. The [476-test source checkpoint](../testing/cyberherd-operator-checkpoint-20261110.md)
records completed Rust/browser/CLI/inactive artifact checks. These interfaces
remain uninstalled/default-off.

Remaining implementation and qualification includes feeder announcement,
lifecycle, roster and operator-interface installed qualification.
Subsequent [failed-cost recovery source](../architecture/cyberherd-failed-cost-recovery.md)
adds protected operator financial reviews, fresh original-payment verification,
append-only budget adjustments and exact payout-role receipts. Preview and
recovery-open cannot initialize or migrate a selected journal; sender credentials
are not loaded for synchronization. No production financial review has been
applied. The [485-test source checkpoint](../testing/cyberherd-failed-cost-checkpoint-20261110.md)
now records completed Rust/root-ownership/browser/CLI/inactive artifact checks.
The legacy pair restore helper rejects CyberHerd schema; its historical process
regression remains covered without claiming multi-journal recovery.

Also required: consistent-restore and retention operations,
installed identity/network/helper/NIP-46/provider qualification, dependency-audit
remediation and explicit financial activation. The twelve numbered groups above
remain the completion contract; a library fixture or green artifact build does
not establish live behavior.

Earlier source-specific evidence remains available in the
[351-test worker](../testing/cyberherd-worker-checkpoint-20261109.md),
[359-test funding](../testing/cyberherd-funding-checkpoint-20261109.md),
[373-test planning](../testing/cyberherd-planning-checkpoint-20261109.md),
[388-test payout qualification](../testing/cyberherd-payout-funding-checkpoint-20261109.md),
[394-test invoice](../testing/cyberherd-outgoing-invoice-checkpoint-20261110.md),
[402-test provider](../testing/cyberherd-provider-checkpoint-20261110.md),
[410-test outcome](../testing/cyberherd-outcome-checkpoint-20261110.md) and
[420-test runtime](../testing/cyberherd-runtime-checkpoint-20261110.md) checkpoints.
Those are historical source/fixture records, not descriptions of unfinished
components that subsequent source has already implemented.
