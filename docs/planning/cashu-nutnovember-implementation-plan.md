# Cashu payments and nutzaps — Nut November implementation plan

**Operator direction:** 2026-11-15.

**Project:** Lightning Goats / Nut November 2026.

**Status:** implementation handoff; not an implementation, deployment, or financial-activation claim.

**Repository baseline inspected:** `54c053b1ae655c829fc6bed96ad59b22b409c082` (`main`). Reinspect the current branch before changing code.

## 1. Goal and scope authority

Build a lightweight, headless Lightning Goats Cashu wallet using external mints. Accept **ordinary Cashu payments and NIP-61 nutzaps**, alongside the existing Strike-backed Lightning payments. All three contribute to the same durable, sats-denominated `herd` feeder-credit pool. When feeding becomes eligible, convert the necessary Cashu holdings into Lightning payments to Strike before physical dispatch. These transfers must never generate another contribution or another credit grant.

Keep the current Lightning LNURL QR in the upper-right corner. Add a reusable Cashu payment-request QR directly below it, aligned to the right edge. Move the existing upper-left credit/status/BTC-price strip to the top center. Preserve the rest of the established artwork and presentation, making only the layout adjustments needed to avoid collisions.

This is the dated, narrow Cashu exception to older documents deferring alternative payment providers and automated conversions. It supersedes earlier Cashu planning suggestions to support nutzaps only, use a single hard-coded mint, require manual-only withdrawals, replace the Lightning QR with a payment-page QR, or depend on a combined QR format. Preserve Strike, Square entitlements, existing CyberHerd work, current opt-outs, live financial limits, and all paid history. Read [AGENTS.md](../../AGENTS.md), the [delivery roadmap](delivery-roadmap.md), and [implementation status](../implementation-status.md).

The operator requested this scoped implementation handoff. Production keys, mint selection, spending/exposure limits, real payments, public receiving announcements, live service activation, schema upgrades, and physical feeding remain explicit operator-controlled steps. Missing production values must not prevent isolated implementation and testing. When placing this document in the repository, link it from `docs/README.md` and add a dated Cashu exception to `docs/planning/delivery-roadmap.md`, preserving all unrelated instructions.

### Required product behavior

| Entry point | Receive path | Contribution rule |
| --- | --- | --- |
| Existing Lightning QR/address, including current Lightning zaps | Existing native Strike invoice/LNURL integration | Authoritative paid receipt creates one contribution, unchanged. |
| New Cashu QR | Reusable Cashu payment request; private HTTPS token delivery | Successfully acquired, persisted ecash creates one contribution. No Nostr account required. |
| Nostr nutzap | NIP-61 relay intake | Successfully acquired, persisted ecash creates one contribution with validated Nostr context. |

Use one threshold, progress display, per-payment cap, and physical owner. A wallet balance is not feeder credit. Ecash is sat-denominated but remains a claim on its mint until redeemed; do not describe acquisition at a mint as receipt at Strike.

### Explicit exclusions

Do not operate a production Cashu mint, require a new Lightning node, replace the Strike backend, implement general customer wallet accounts, publish spendable tokens in a QR, or build a browser-hosted treasury. Full NIP-60 wallet synchronization, arbitrary mint auto-trust, inter-mint swaps, Cashu payouts to CyberHerd members, a third nutzap QR, unified LNURL/Cashu experimentation, and a general website redesign are outside this milestone. A local mock mint or disposable protocol fixture for tests is not a production-mint requirement.

## 2. Repository integration points

Inspect these existing components before creating parallel machinery:

| Existing component | Work to reuse or extend |
| --- | --- |
| `src/ledger/mod.rs`, `src/ledger/credit.rs` | Atomic contribution/credit records and asset provenance. Add a typed Cashu receipt path; use verified Cashu provenance. |
| `src/ledger/operator_credit.rs` | Reuse its transactional/idempotent design, not its administrative capability or event type. Do not expose operator grants as a public payment endpoint. |
| `src/ledger/feed.rs`, `src/ledger/feed_cap.rs` | Thresholds, caps, durable attempts, confirmation-only debits, and unresolved physical-attempt protection. Add pre-dispatch funding reservation/settlement. |
| `src/ledger/reward_funding.rs` and related payout modules | Preserve source evidence, fee budgets, reservations, and actual consumed funding. |
| Existing Strike receive adapter, inbox, and recovery workers | Add purpose-bound treasury invoices and reconciliation without a second independent Lightning receive implementation. |
| `src/zap.rs`, existing Nostr verification/signing/outboxes | Reuse bounded event verification and publication patterns; keep NIP-57 and NIP-61 distinct. |
| `web/overlay.html`, `web/overlay.css`, `web/overlay.js` | Add the Cashu QR and top-center strip; preserve sequence-aware snapshots and existing effects. |
| Existing migrations, systemd/edge configuration, maintenance tools and tests | Extend additively, including coordinated wallet/core recovery. |

The inspected feed code currently gates a physical attempt using aggregate credit and treats an unresolved physical attempt as globally blocking. **Do not insert a mint wait into that physical-attempt state.** Introduce a separate financial reservation state before physical dispatch, and make credit selection, confirmation debits, caps, and reward provenance agree on the same frozen source allocations.

The current reward code derives authorization from verified Strike funding. Merely putting a new source string into a settlement row does not implement Cashu support. The current status document also records recipient-relay routing and CyberHerd lifecycle/stream-chat opt-outs; do not undo these while adding nutzap messages.

## 3. Architecture and authority boundaries

```text
Existing Lightning LNURL / NIP-57 ------> Strike receive adapter --------+
                                                                       |
Cashu request -> HTTPS token inbox --+                                  v
                                     +-> isolated Cashu wallet -> typed receipt import
NIP-61 events -> Nostr intake --------+                                  |
                                                                        v
                                                           shared herd credit ledger
                                                                        |
                                                         pre-feed funding reservation
                                                                        |
                                   Cashu wallet pays purpose-bound Strike invoice(s)
                                                                        |
                                      core independently verifies Strike receipt(s)
                                                                        |
                                                        existing gateway / physical owner
                                                                        |
                                                confirmed feed debit and presentation
```

Implement one small wallet service, proposed name `lightning-goats-cashud`, with a protected durable database and keys. The public receiving endpoint is a bounded token-submission interface, not a mint API, an arbitrary payment API, or a generic wallet RPC. Prefer the existing public daemon/edge as a constrained ingress with authenticated local IPC to the wallet; do not introduce multiple unnecessary services.

Evaluate CDK's wallet components first [S11]. Pin the selected version and test its actual persistence/recovery behavior. Preserve the repository's Rust toolchain and reviewed SQLx/SQLite constraints unless an explicit compatibility decision is documented. A separate crate/package or dependency boundary is acceptable where it prevents an unrelated upgrade of the running core. Do not add a mint binary or implement Cashu cryptography from scratch.

The wallet necessarily has authority to spend its own ecash. Isolate it under a separate non-admin UID with protected configuration, wallet state, seed, and dedicated receiving key. It must not have Strike withdrawal/payout credentials, the OpenHAB token, arbitrary household network access, the project Nostr private key, or write access to the core ledger. The core remains the authority for credits, treasury destinations, and physical dispatch.

Use a versioned Unix-socket protocol with peer-UID checks on the same host, protected socket directories, bounded messages, and durable request IDs. Separate receipt submission from treasury instructions. The core creates and independently verifies Strike invoices with its existing receive-only authority; it sends the wallet a frozen invoice binding and approved amount, not a generic caller-supplied destination. No public endpoint may cause arbitrary melts, refunds, transfers, or credit grants.

Keep the Cashu seed/P2PK key separate from the project's Nostr identity. Reuse the existing NIP-46/bunker boundary for narrowly authorized public discovery and redemption-history events. Qualify the necessary event kinds and encryption operations; do not fall back to storing the project nsec on the VPS.

## 4. Mint configuration and protocol compatibility

Support multiple compatible **configured** mints from the start, with separate balances, operation journals, and limits per mint. A second mint must be addable through protected configuration, not source-code changes. Start a live pilot with the explicitly approved mint set; demonstrate a second configuration in fixtures or an independently authorized live test.

For each mint retain an internal stable identity, exact advertised URL, unit, pinned/verified keyset evidence, enabled receiving paths, and capability checks. Inspect support for swapping, BOLT11 melting, state checks, recovery, and P2PK/DLEQ where required. Account for keyset input fees and minimum/maximum melt amounts. A compatibility declaration is not proof of reliable behavior; record tested versions and results [S2–S10].

Use a strict acceptance list. Never follow an untrusted event/token URL to discover or approve a mint automatically. Apply TLS verification, bounded responses, DNS/IP validation, redirect controls, and private/link-local/loopback/metadata-address blocking to every mint request. Test IPv4, IPv6, mixed DNS answers, DNS rebinding, and URL aliases. Maintain one canonical identity per explicitly approved mint; do not create double-spend domains from superficial URL variations or merge distinct issuers by guesswork.

Freeze configuration/version information on admitted receipts. Removing a mint from new acceptance must not discard existing holdings, pending operations, old receiving keys, or their recovery paths. Bound unknown-mint submissions without retaining unbounded attacker data. Reject before acquiring unsupported funds; preserve and visibly reconcile any funds already acquired.

Before live activation select exposure ceilings, receipt/proof/body limits, maximum operation duration and retries, settlement fee ceilings, maximum holding time, and sweep thresholds. Use disabled execution/zero spending permission in example configurations, not invented live financial limits.

## 5. Ordinary Cashu receiving and reusable QR

Use NUT-18 receiving instructions over HTTPS POST [S2]. The request is reusable (`s=false`), has no fixed amount, declares `u=sat`, explicitly lists approved mints, and keeps that list strict. The request description identifies Lightning Goats and the shared herd contribution. Use a short first-party transport URL. Require the selected mints to support our BOLT11 exit path even where a sender does not understand optional request capability fields.

The Cashu QR carries receiving instructions, **never a token containing funds**. Select the displayed NUT-18 `creqA...` or NUT-26 `CREQB1...` encoding through actual wallet scan tests [S2, S3]. Prefer the QR-efficient encoding when the qualified clients support it. Preserve case-sensitive base64 payload bytes; do not uppercase a `creqA` payload using the existing Lightning QR helper. A fallback copy/request view may be added without replacing either overlay QR.

A reusable request's identifier identifies the receiving descriptor, not a donation. Multiple independent payments to the same descriptor must all work. Version descriptors when mint/key/transport policy changes; retain a documented old-descriptor handling window and the keys necessary for admitted work. Do not rotate a reusable QR for each donation.

Accept standard private Cashu payloads through this path without Nostr authentication. Support ordinary bearer proofs and the explicitly supported receiver-only P2PK profile; do not require a social identity. Verify supplied DLEQ evidence and reject invalid evidence. Missing DLEQ on an ordinary private payment must follow an explicit tested policy, with no credit before a successful mint swap into our own outputs. NIP-61 has the stricter mandatory proof checks below. Unsupported spending conditions are not silently ignored.

The endpoint must provide bounded durable admission, truthful pending/accepted/rejected outcomes, and an unguessable status capability where asynchronous completion requires it. An HTTP acknowledgment is not financial settlement. Test response behavior with real sender implementations, including lost replies and repeated POSTs. Do not invent a protocol-standard callback the selected wallets do not implement.

Browser-based Cashu wallets may POST from another origin. Qualify OPTIONS/CORS on only the public token-receive/request routes, without cookies or credentialed cross-origin sessions. Do not apply permissive CORS to administrative/credit/spending interfaces. Prevent token bodies, secrets, status capabilities, or memos from appearing in access logs, WAF body logs, analytics, error reports, or URLs.

## 6. NIP-61 receiving

Publish a correctly signed `kind:10019` with approved mints, receive relays, and the dedicated P2PK key. Subscribe for relevant `kind:9321` events. Validate signature/ID, recipient, mint, unit, proof bounds, DLEQ, and locking conditions before acquisition. Apply NIP-61's public-key encoding rules with library-backed key handling and interoperability tests, not ad hoc string manipulation [S1, S4, S5].

For the first release accept the simple receiver-controlled P2PK profile. Reject unsupported refund branches, extra required signers, timelocks, and signature flags rather than interpreting them permissively. Verify mint enforcement capability. An event referencing a goat or note cannot select a different credit pool or arbitrary payout destination; initial Cashu contributions go to `herd`.

Use durable relay intake with bounded overlapping backfill. Event timestamps and `since` cursors are discovery hints, not uniqueness or payment evidence. Delayed/reordered events and reconnects must not duplicate receipts or discard admitted work. Keep old authorized receiving keys available for recovery under their historical policy.

After acquisition, implement NIP-61 redemption-history signaling (`7376`) through the restricted signer and exact-byte retry outbox. Preserve its privacy semantics and publish to appropriate sender read relays using the existing bounded routing infrastructure. Do not invent a `7375` token-event reference when no such event exists. Full NIP-60 synchronization remains unnecessary [S1].

Nutzap messages may identify the validated event author, but this does not establish the economic origin of the money. Plain Cashu remains unattributed unless a separate authenticated association exists. Do not automatically grant CyberHerd membership, reputation, or an extra NIP-57 receipt merely because a Cashu payment arrived. Preserve current publication settings and opt-outs.

## 7. One acquisition engine and durable receipt import

Both Cashu transports must enter the same acquisition engine and proof index. Suggested states are:

```text
observed -> validated -> acquisition_prepared -> acquiring
         -> acquired -> credit_export_pending -> credited
                         \-> held_for_reconciliation
```

Before any state-changing mint request, persist its operation ID, exact input identities, selected keysets, output secrets/blinding data or deterministic derivation counters, and exact request material. Reserve wallet inputs transactionally with a single active writer/spend coordinator. Persist and verify returned outputs before reporting acquisition.

Use tested state-check, signature-restore, and supported cached-response behavior to recover interrupted operations [S6–S8]. A lost response is not failure. `SPENT` input state alone is not evidence that our wallet obtained the output value. If ownership of the resulting funds cannot be established, hold the operation with no new credit and no physical dispatch. Cached responses have endpoint/retention limits; do not assume infinite recovery from a transient cache.

Deduplication must cover:

- Nostr event ID for relay replay.
- Canonical mint/unit plus proof identity, based on the Cashu `Y`/secret identity, across both HTTP and Nostr paths. Record keyset/signature evidence as immutable associated data; changing wrapper, order, memo, witness, or event ID must not create new money.
- Acquisition ID and immutable receipt digest for wallet-to-core retries.
- Treasury transfer, Strike receive ID, payment hash, and source-allocation identities for settlement.

Validate before an attacker can permanently claim a proof identity with a malformed message. Duplicate proof entries inside one payload and partly overlapping old/new proof sets must not create partial accidental grants. Define all-or-nothing admission or an explicit recoverable split with deterministic credit attribution; never silently discard fresh funds. Preserve dedupe history after spending and after restarts.

A typed private receipt should bind schema version, wallet instance/epoch, acquisition ID, transport, original source reference, mint/unit, gross amount, receive fees, net acquired amount, acquisition evidence digest, policy version, and `herd` destination. Do not transmit spendable outputs to the overlay or core public events. The wallet is a trusted payment verifier, not a trustless oracle; contain compromise with authentication, exposure and credit-rate limits, and reconciliation.

Import receipt provenance, the capped credit grant, and its public event in one core transaction. Same receipt plus same data returns the original result; conflicting reuse is held/rejected without mutation. Deliver/ack through a durable outbox/inbox; an acknowledgment loss must be safe after the credit has already been consumed. Neither callbacks, relay receipts, nor wallet balance deltas can grant credit.

## 8. Credit amounts, caps, fees, and conservation

Use checked integer sats throughout. Preserve gross received value, receive/input fees, net acquired ecash, credited value, capped/retired credit, remaining holdings, reservations, treasury deposits, and actual settlement costs as different fields.

**Implementation policy:** initial credit equals net successfully acquired ecash after receive fees, followed by the existing cap policy. Later conversion fees do not reduce an already granted credit balance. They are bounded project expenses recorded separately. This provides the same feeding treatment per credited sat without promising identical costs for different payment methods.

Preserve the existing [three-feed-per-payment and one-announcement policy](../deployment/per-payment-feeder-cap.md), partial-credit behavior, audited retirement of excess, and original payment provenance. A valid new acquisition is one payment for cap purposes; replaying or rewrapping its proofs cannot reset the cap. Genuine separate donations remain separate payments subject to the shared physical limits. Retiring excess feeder entitlement does not destroy or refund the actual ecash holdings.

Maintain explicit conservation equations, including:

```text
gross received = net acquired + receive fees
melt inputs = verified Strike deposit + actual melt fees + recovered change
credit after settlement = credit before settlement
confirmed-feed debit = the existing configured threshold, exactly once
```

Track pending/reserved proofs and unrecovered change separately; do not force an uncertain transaction to balance by inventing a fee or write-off. Ensure source-attributed amounts cannot exceed actual acquired value. Keyset/denomination rounding, fee reserve, and returned change need deterministic allocation.

A fee reserve can require more input value than the final fee. Do not borrow already reserved donor funds or assume a 400-sat holding can pay a 400-sat invoice plus fees. The default is an affordable net Strike deposit from the assigned holdings, with the difference recorded as the bounded conversion expense and the original feeder credit preserved. A separately recorded operator fee top-up is permissible only under an explicit policy; it must not create a donation or reward opportunity. Reject/hold uneconomic operations, excessive fee quotes, and amounts below supported melt minimums without silently erasing credit.

## 9. Feeding-triggered settlement and outage behavior

Separate financial reservation from physical dispatch:

1. Credit reaches the configured threshold using Lightning and/or acquired Cashu.
2. The core durably selects funding lots and freezes their allocation to a proposed feeding. Cashu settlement jobs reserve the actual wallet inputs needed for those portions.
3. Existing Lightning portions, and previously verified Cashu-to-Strike portions, need no further transfer.
4. For each required mint, obtain a purpose-bound Strike invoice, create a bounded melt quote, execute once, and reconcile the original operation.
5. Independently verify Strike receipt and complete source coverage/fee accounting. Only then mark the proposed feeding financially ready.
6. Recheck current feeder availability, override, cooldown and limits; enter the existing physical-attempt protocol.
7. Debit credit only on correlated confirmed feeding. Preserve unknown physical outcomes and existing reconciliation safeguards.

Use one authoritative lot-allocation mechanism for reservation, caps, feed debit, and reward evidence. Preserve historical FIFO evidence. For new work, select deterministically within eligible/unreserved lots and document any eligibility-aware departure from strict global FIFO. Do not debit a different lot at confirmation than the one settled/reserved beforehand.

**Avoid cross-rail head-of-line blocking:** a mint outage must not prevent receiving/crediting Lightning or dispatching an independently funded, settled-only feeding. Financially pending Cashu must not create a globally unresolved physical attempt. Implement an eligible-funds selection path and test a held earlier Cashu lot followed by enough Lightning funds. If insufficient settled funds exist, the affected feeding waits truthfully. Physical uncertainty still blocks physical dispatch exactly as before; this is not permission to bypass the owner.

Use separate invoices/jobs per mint for the initial release. Do not depend on several mints coordinating payment of one invoice. Bounded batching within a mint is allowed when the receipt-to-transfer allocation remains explicit, and may reduce uneconomic tiny melts. A completed transfer is never repeated because another mint's transfer is still pending.

Feed-triggered conversion is the primary trigger. Add configurable maximum holding age, exposure/balance thresholds, and an operator-controlled sweep using the same constrained machinery. These handle below-threshold donations, retired excess, and periods when feeding is paused. An unattended sweep is a treasury event, not a feed or donation. Coordinate sweeps and feed settlements through the same proof reservations.

If settlement completes but feeding is disabled, the credit remains available and already settled. If a feed is definitively not dispatched, safely release only the relevant unconsumed reservation; preserve all transfer history and do not melt again. Unknown physical execution keeps its original request ID. A pause or a restart never authorizes an automatic refund.

## 10. Strike treasury-transfer contract

Add an explicit immutable receive purpose, proposed `cashu_treasury_settlement`, alongside existing donation/contact purposes. Persist its intent before requesting an invoice and bind the returned receive ID, hash, invoice, BTC amount, network, destination/account, and source allocation **before exposing the invoice to the wallet**. Use BTC amounts/target currency for this path; verify actual provider behavior instead of introducing fiat conversion [S9].

The existing Strike webhook inbox and startup/periodic recovery must route treasury-purpose receives away from ordinary donation grants. Unknown or conflicting mappings are quarantined, not guessed to be donations. A mint's paid response or preimage alone is insufficient: the core checks the intended Strike receive resource and actual completed amount independently.

Journal creation, quote, submission, pending outcome, provider verification, fee/change reconciliation, and allocation completion. Freeze the quote ID, invoice hash, proofs, and recovery identity before submission. Never issue a new payment attempt for an unresolved original operation. Invoice expiry does not prove that a submitted Lightning payment failed. A replacement requires a verified terminal nonpayment/no-submission outcome, safe proof recovery, and an explicit linked supersession record. Do not assume the Strike API implements an idempotency key without checking its documented behavior.

Recover orphaned invoice-creation results safely without treating them as donations. Do not expose an unbound invoice. Handle webhook-before-worker-response ordering, duplicate callbacks, completed Strike receipt with mint reply loss, mint `PENDING`, and late settlement after a local timeout [S10].

The following must all be zero for an internal transfer: new feeder credits, new donation totals, payment celebration count, per-payment cap reset, and newly manufactured reward opportunities. Keep internal receive evidence private while exposing only coarse operational status.

## 11. CyberHerd and messaging compatibility

Keep incoming contribution identity separate from its eventual settlement deposit. Existing Lightning membership and payout behavior must continue unchanged. Do not represent an ordinary Cashu payment as a Nostr-authenticated zap, and do not enable lifecycle/stream-chat output that the operator disabled.

Cashu at a mint contributes **zero** to existing Strike spending authorization. Plan a typed `cashu-settled-to-strike` provenance extension if Cashu-funded feedings are to participate in CyberHerd rewards: bind the original acquisition, actual consumed portion, verified net Strike deposit, allocated conversion costs, and one-time financial reservation. Never relabel the original receipt as a direct `strike_btc` donation.

Implement/test that extension as a separately gated slice. Its budget must not exceed the deterministically attributed verified net funds and must preserve the current inclusive payout-fee policy and limits. No double reward arises from the nutzap event, core contribution, transfer, or physical confirmation replay. Pending/partly settled sources cannot authorize payout. Enabling this capability remains separate from enabling Cashu acceptance; until qualified and explicitly enabled, mixed feeds authorize only their existing eligible Strike share. Document that limitation rather than silently promising reward parity or blocking the receiving milestone.

Use existing payment/goat-fact and confirmed-feeding presentation. Add a transport marker such as Cashu/Nutzap without exposing proofs, wallet balances, invoice IDs, private mint-operation evidence, or untrusted markup. Shared progress reflects authoritative credit, not receipt events accumulated in JavaScript. Add coarse funded/settling/ready/awaiting-physical-confirmation states; a full bar must not falsely claim the goats have already been fed.

## 12. Overlay implementation contract

The layout decision is final for this milestone:

```text
                           top center                         upper right
                      [credit/status | BTC price | change]   [existing LIGHTNING QR]
                                                              [CASHU request QR]

[goat / CyberHerd cards placed clear of the payment QR column]

                     existing stream / effects

                     existing bottom progress bar
```

Here, the element to center is the current `#top-left-container` strip containing `#sats_left`, `#btc-price`, and `#btc-change`, not the Lightning QR. Keep the existing Lightning QR in the upper-right corner. Preserve its reusable LNURL and `herd@lightning-goats.com` receiving path; regression-test the existing payload and endpoint, rather than substituting BOLT11 or a web URL.

The operator supplied this existing clickable Lightning URI; retain it as a regression fixture, confirming the QR's actual encoded payload separately:

```text
lightning:LNURL1DP68GURN8GHJ7MRFVA58GMNFDENJ6EM0V968XTNRDAKJ7TNHV4KXCTTTDEHHWM30D3H82UNVWQHKSETJVSLH78D5
```

Add independent Cashu QR/data/label elements with immutable display snapshots and a safe absent/disabled state. Show the `Lightning` and `Cashu` labels directly below their respective codes. No third QR, cycling between methods, or forced payment-page navigation. A short shared caption is optional only if it remains legible and does not obscure the scene.

Preserve the fixed 1920x1080 composition and `?fit=1` scaling. Keep the center strip clear of the right-side payment QR column. Keep the goat and CyberHerd cards clear of both the QRs and central notices. Test all goat cards, the actual three-member CyberHerd roster, connection labels, paused/unknown banners, long status text, and payment/feed effects together. Do not hide cards, disable banners, shrink text excessively, or change artwork to conceal overlap.

Use matching 180px square white QR panels in the native overlay, with rounded corners and a visible gap below the Lightning label so a phone can point at either code. Keep the four-module quiet zone inside the panels and keep the Cashu code free of logos. Verify both QR payloads after browser rendering and video downscaling, including 1080p and a 720p stream rendition. Record actual phone/wallet scan results separately from automated decode tests.

Keep both codes visible in the normal resting overlay. Avoid new animation that replaces/obscures a code, and preserve/reconcile the existing transient Lightning effect deliberately. Feature-off and Cashu-unavailable states must retain the working Lightning QR. A paused feeder is different from a disabled payment rail: preserve current accepted-credit behavior and describe waiting honestly.

## 13. Security, operations, migration, and recovery

Threat-model public token submission, proof-copying, overlapping inputs, malicious mints, SSRF, replay, database rollback, wallet theft, and physical double actuation. Bound request size, proof count, CPU-heavy validation, relay backfill, concurrent jobs, and per-mint retries. Use scoped WAF rules with redacted request bodies, not a payment-wide bypass. Never echo token secrets in errors.

Expose protected operator commands for status, held operations, mint-policy updates, descriptor/announcement preparation, constrained settlement, reconciliation, and backup verification. Show useful receipt IDs and coarse causes without dumping secrets. Include wallet/core epoch mismatches, pending age, custody exposure, fee usage, and queue lag in private health checks.

Additive migrations must preserve all original paid rows, source IDs, cap/retirement history, signed outboxes, feeder UUIDs, and unrelated journals. Extend the existing maintenance/backup inventory to include wallet state, operation material, seed/key backups, and cross-store acknowledgments. A seed alone does not reconstruct every pending P2PK receipt and core credit decision; document exactly what is recoverable from each backup component.

Before schema maintenance, quiesce all affected writers and verify backups. Test wallet-only rollback, core-only rollback, and coordinated restore. A restored process must reconcile mint/Strike state and ledger epochs before resuming new credits or spends; never restore yesterday's unspent list and spend it blindly. Keep older accepted contributions and financial/physical uncertain work fenced until reconciled.

Runtime toggles must separate receiving/public discovery, credit import, Cashu settlement spending, and Cashu-derived reward authorization. Disabling new work must not delete admitted funds or prevent read-only recovery of already submitted operations. Rollback means disabling the new feature and retaining financial state, not undoing external transfers or restoring a database over newer paid history.

Do not change production DNS, household network topology, OpenHAB rules, keys, or existing service identities merely to add the wallet. Reuse the narrow physical gateway. Verify existing Lightning routes, WebLN/NIP-57, private contact payments, Nostr publication settings, and overlay reconnect behavior after every relevant integration slice.

## 14. Ordered coding-agent work packages

| Slice | Deliverable | Exit evidence |
| --- | --- | --- |
| 0 — baseline and compatibility | Inspect current source/ongoing work; record existing schema/tests; pin CDK/protocol revisions; choose recoverable wallet storage; document mint/wallet matrix and fee model. | Reproducible build spike and written contract, with no live mutations. |
| 1 — wallet journal | Isolated wallet, protected keys, proof reservation/dedupe, swap recovery, status and backup support. | Restart, lost-response, overlap, corrupted-state and conservation fixtures. |
| 2 — both intake paths | Reusable NUT-18 HTTP receiving plus NIP-61 intake/discovery/redemption signaling using one engine. | Ordinary payment and independent-client nutzap each acquired once; cross-transport replay rejected. |
| 3 — core contribution adapter | Authenticated receipt import, Cashu provenance, shared cap/progress/events, new financial states. | Exactly one grant across IPC retries, mixed-rail contributions, and preserved Lightning history. |
| 4 — settlement and feed gate | Purpose-bound Strike invoices, melt/change reconciliation, source coverage, eligible-lot selection, fallback sweeps. | Verified deposits without new credit; mixed feeding progresses; mint failure does not block settled-only feeding. |
| 5 — overlay and edge | Cashu below Lightning in the upper-right, status strip top-center, cards and banners clear of both QRs; CORS/WAF and safe public state. | Browser geometry/decode tests and real scan matrix. |
| 6 — operational qualification | Additive migration/restore tests, inactive install, risk-focused integration, optional gated Cashu-derived reward provenance. | Source-pinned acceptance matrix and operator-ready deployment runbook; feature states explicit. |
| 7 — approved pilot and submission | Only after approval: configure keys/mints/limits, announce receiving, conduct bounded real acceptance, capture demo and submission evidence. | Truthful live results distinct from fixtures; tagged November contribution. |

Keep changes reviewable. Reuse existing coordination rather than inventing an agent framework. Do not abandon either Cashu receiving path to finish presentation polish. Do not replace required financial tests with a mock-only successful demo. If a concrete defect blocks one path, hold that path and report it while continuing independent safe work.

## 15. Mandatory acceptance matrix

| Scenario | Required result |
| --- | --- |
| Existing LNURL, Lightning/NIP-57 payment and restart | Original destination, one contribution, original cap/feeding behavior. |
| Several users pay the same reusable Cashu request | Every distinct acquired payment works; descriptor ID is not donation ID. |
| HTTP retry / several relays / same proofs wrapped as another event | One acquisition and one grant, even after credit consumption. |
| Same proofs cross HTTP and nutzap paths | No double credit, cap reset, or attribution-based duplicate. |
| Duplicate/overlapping proofs; forged signatures; bad unit/keyset/conditions | No accidental partial credit, resource exhaustion, or arbitrary mint fetch. |
| Zero/dust/huge values; many tiny fee-bearing proofs; integer overflow | Deterministic bounded rejection/hold with preserved acquired funds. |
| Two configured mints and keyset rotation | Separate valid balances/recovery; no provider hard-coding. |
| Crash at each swap boundary; reply lost after mint execution | Recover original owned outputs or remain held; no guessed payment. |
| Credit committed, acknowledgment lost, worker restarted | Retry returns same receipt and does not re-credit. |
| 600 Lightning + 250 nutzap + 150 Cashu, threshold 1000, zero-fee fixture | Credit 1000 once; settle Cashu portion; one confirmed feed; credit zero. |
| Same mixed example with nonzero fees/change | All fees/change conserved, original granted credit stable, no invented treasury value. |
| Treasury invoice webhook before melt reply, duplicate webhook, late receipt | Correct internal purpose; zero donation credit/celebration; original operation reconciled. |
| One mint settles, another remains pending | Completed portion never repaid; affected feeding waits; funds preserved. |
| Earlier held Cashu, later sufficient Lightning | Lightning receiving and settled-only feeding continue without consuming held funds. |
| Sweep races feeding, duplicate processes, simultaneous melt requests | Single reserved spend per proof and consistent source allocations. |
| Settlement done, feeder paused/not dispatched/unknown | Credit retained or consumed only on actual confirmation; no repeat melt or fresh-ID ambiguous retry. |
| Above-cap donation and remainder | Current cap/retirement/announcement semantics; excess holdings still accounted. |
| Mint removal, old QR, delayed nutzap, unavailable relay | Explicit admission/recovery policy; no silent loss or auto-trust. |
| Cashu funds present but unverified at Strike | No new Strike payout authorization. |
| Qualified Cashu-derived reward path enabled | Bound to net verified deposit and consumed contribution; one reward, no transfer-as-donation. |
| Browser-wallet cross-origin POST and WAF | Compatible legitimate flow; bounded malicious requests; token bodies absent from logs. |
| QR encoding/case, long request, 1080p/720p and `fit=1` | Correct decode and recorded real scans; no card/banner/strip overlap. |
| Feature disabled or mint outage | Existing Lightning UI/receiving intact; honest Cashu status. |
| Restore older wallet/core snapshot; lost key/backup component | Startup/recovery fences prevent duplicate spends, grants, rewards, or feeding. |

Run the repository's required checks on the selected source, including `cargo fmt --all --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings`, and `cargo test --locked --all-features`, plus relevant browser, security, migration and deployment suites. Record failures/skips accurately. Test deployed/provider behavior only with authorized credentials and funds. No fixed live donation quota or unrelated whole-project acceptance ceremony is introduced by this plan.

## 16. Deliverables and definition of done

Deliver source, additive migrations, pinned dependencies, safe example configuration, inactive service/edge artifacts, operational commands, and automated tests. Add focused architecture, acceptance, and deployment documents under the existing docs structure. Update implementation status only with the exact demonstrated state; do not convert this plan into a claim that a service is installed.

The receiving milestone is complete when both ordinary Cashu and NIP-61 nutzaps from qualified independent clients create exactly one contribution through the common engine; Lightning remains unchanged; Cashu settlement into Strike is verified without new credit; a correctly funded feeding uses the existing physical protocol; and the requested two-QR layout passes scan and collision checks. Production acceptance additionally requires the approved real-provider, backup/recovery, and physical evidence. Cashu-derived CyberHerd reward support has its own explicitly recorded qualification/activation status.

Before live enablement, the remaining operational selections are the exact mint set, client versions, key provisioning, limits/fee ceilings, holding-age policy, account binding and approved rollout. These are deployment inputs, not reasons to reopen the chosen product/layout or stop repository development.

## 17. Nut November package

The official event currently closes **2026-11-27 at 23:59 UTC (16:59 America/Denver)**. Existing projects must distinguish November additions from pre-November work; code must be public and open source. Coding agents are permitted [S12]. Recheck the official submission instructions before submitting.

Record both the pre-November reference and this workstream's starting revision. Package a tagged release/PR set, concise November change inventory, setup instructions, compatibility matrix, architecture/limitations, and acceptance results. Record a short demonstration of ordinary Cashu and nutzap contributions, unchanged Lightning, one shared progress bar, settlement, and duplicate replay producing no additional credit. Show physical feeding only when authorized and actually observed; label simulated footage/results honestly. Avoid extra animal feedings for repeated takes.

The final report must distinguish source complete, fixture-qualified, installed, financially enabled, and live-verified. Deadline pressure is not permission to remove safety controls or claim universal wallet compatibility.

## 18. Primary references

Protocol and provider documents were checked on 2026-11-15. Pin the revisions used for implementation; evolving optional features require actual interoperability checks, not assumptions from a standards page.

- **[S1]** [NIP-61: Nutzaps](https://nips.nostr.com/61).
- **[S2]** [NUT-18: Payment requests](https://cashubtc.github.io/nuts/18/).
- **[S3]** [NUT-26: Payment-request Bech32m encoding](https://cashubtc.github.io/nuts/26/).
- **[S4]** [NUT-11: P2PK spending conditions](https://cashubtc.github.io/nuts/11/).
- **[S5]** [NUT-12: DLEQ verification](https://cashubtc.github.io/nuts/12/).
- **[S6]** [NUT-07: Proof-state checks](https://cashubtc.github.io/nuts/07/).
- **[S7]** [NUT-09: Signature restoration](https://cashubtc.github.io/nuts/09/).
- **[S8]** [NUT-19: Cached responses](https://cashubtc.github.io/nuts/19/).
- **[S9]** [Strike: Create a receive request](https://docs.strike.me/api/create-a-receive-request/).
- **[S10]** [NUT-05: Melting](https://cashubtc.github.io/nuts/05/) and [NUT-23: BOLT11](https://cashubtc.github.io/nuts/23/).
- **[S11]** [Cashu Development Kit](https://cashudevkit.org/).
- **[S12]** [Nut November rules and timeline](https://nutnovember.org/).
