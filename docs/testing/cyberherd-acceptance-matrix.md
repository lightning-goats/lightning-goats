# CyberHerd full-port acceptance inventory

This inventory follows all twelve requirements in the
[implementation plan](../planning/cyberherd-implementation-plan.md). It is not a
completion declaration. A test file identifies coverage to inspect; its existence
alone does not prove a passing test or live acceptance. Historical live checkpoints
prove their recorded observations, not that today's service configuration is the
same. Current production review and qualification are recorded in the
[November 13 checkpoint](cyberherd-inclusive-upgrade-checkpoint-20261113.md).

The [approved additive payout-schema upgrade](cyberherd-inclusive-upgrade-live-20261114.md)
is installed with unchanged configuration and disabled outgoing roles. It does
not complete the full port.

The [November 14 program upgrade and future-only cutoff](cyberherd-initial-reward-cutover-live-20261114.md)
are also complete. All 20 historical confirmed feedings, including five deferred
inputs, are durably excluded without payouts. The native comparison preserved all
pre-existing rows; outgoing roles remain masked. This resolves historical reward
disposition, not provider acceptance or financial activation. The same checkpoint
records read-only verification of the current signed announcement/goal and current
three-member settings; remaining zap, stream and lifecycle scopes stay open.

The separately approved [live financial activation](cyberherd-financial-activation-live-20261114.md)
has now completed against a fresh native checkpoint. All four services are
running/enabled at the selected limits and all 20 historical barriers remain
excluded. This establishes activation, not a successful real payout or external
recovery adoption. Public member/lifecycle publication was disabled at that
activation checkpoint. The later [public messaging rollout](cyberherd-discovery-repair-live-20261114.md)
established real note/lifecycle relay receipts. The operator then
[disabled CyberHerd stream chat](cyberherd-no-stream-chat-20261114.md).

The subsequent [admission repair](cyberherd-conor-admission-20261114.md)
processed retained likes/reposts through the normal worker and restored ordered
progress without inserting members. The latest
[zap messaging release](zap-payment-messaging-checkpoint-20261114.md) is installed:
all four services are running, notes/lifecycle are enabled, chat is disabled,
and the public roster is ready with three active members and a 10% allocation.
Its exact source passed 432 library tests, integrations/root checks, security and
native systemd rehearsal. Read-only live financial observation found two
`no_payout` reward plans, zero payout grants/rows and no provider payment records.
No actual payout or externally observed zap-aware message is claimed.

Read-only [current-member recipient qualification](cyberherd-recipient-success-status-20261114.md)
found two valid recipients and an explicit-success parsing defect for Primal.
That correction is in progress; the failed recipient must not be counted as
qualified. The same checkpoint records two correctly empty frozen no-payout plans
and one retained America/Denver midnight reset receipt. No payments, feeds or
membership resets were submitted for these observations.

| Requirement | Source and recorded evidence | Remaining acceptance |
| --- | --- | --- |
| 1. Membership and arithmetic | `src/cyberherd/store.rs`, `shares.rs`; [live social intake](cyberherd-live-social-intake-20261110.md) records reaction/repost admission and restart retention. | Three-user capacity and the ready three-member roster were revalidated after the latest deployment. Exercise settled zap increase/displacement and rejection rules on the installed stage. NIP-05 remains unnecessary. |
| 2. Points and shares | `src/cyberherd/shares.rs`, `ingress_tests.rs`; social checkpoint records exact live scores/shares. | Check the complete once-per-cycle/pre-join/re-entry and largest-remainder cases in exact-source test results; correlate paid-member shares with authoritative settlement on the explicitly activated stage. |
| 3. Durable cycles | `src/cyberherd/store.rs`, `operator_tests.rs`; transactional/restart fixtures are part of Rust qualification. | Verify selected installed Mountain-time reset/catch-up behavior and operator idempotency without resetting paid history. A membership-only restart does not prove every reset path. |
| 4. Targets and profiles | `src/cyberherd/targets.rs`, `tests/cyberherd_targets.rs`; public leaderboard checkpoint records authenticated target links. | Verify current-day note, stream versions/aliases, linked goal and stale/ended precedence on the selected runtime; retain signature/host-authority rules. |
| 5. Ingestion and pause recovery | `src/cyberherd/worker.rs`, `ingress_tests.rs`; live social checkpoint records bounded recent/historical recovery and no duplicate awards. | Verify current installed override/defer/refresh/replay/discard behavior, including discovery unavailable and active ordinary zap eligibility. |
| 6. Incoming NIP-57 | `tests/zap_receive.rs`; [receipt checkpoint](cyberherd-zap-receipt-checkpoint-20261110.md). | Complete externally observed zap settlement, exact description-hash binding, durable receipt and exactly-once membership consumption. Ordinary Lightning receipt is insufficient proof. |
| 7. Confirmed-feeding rewards | `src/cyberherd/rewards_tests.rs`, `reward_store.rs`; [funding](cyberherd-payout-funding-checkpoint-20261109.md) and [inclusive fees](cyberherd-inclusive-fees-checkpoint-20261111.md). | The journal upgrade and historical cutoff are complete; qualify future real funding/recipient outcomes. Current readback has two no-payout plans and no executed payout. Only confirmed physical UUIDs earn rewards; virtual credits and threshold messages do not. |
| 8. Isolated outgoing payments | `src/payout/worker_tests.rs`, `provider_workflow_tests.rs`, `cost_recovery_tests.rs`; [recipient qualification](cyberherd-recipient-checkpoint-20261110.md). | Real provider/recipient acceptance, inclusive fee deduction, retained original-ID reconciliation and external restore adoption remain open. Live execution was explicitly approved separately; synthetic tests or selected settings do not prove real outcomes. |
| 9. Messages and delivery | `src/cyberherd/delivery_tests.rs`, `result_tests.rs`; [member delivery](cyberherd-member-delivery-checkpoint-20261110.md). | Installed member note/overlay output is recorded in the public rollout. Verify actual treat output and remaining frozen increments/footer/retry scopes. Zap-template custom-edit/opt-out persistence passed exact-source tests; real zap-aware output remains unobserved. No treat notice precedes authoritative payout settlement. |
| 10. Conversation destinations | `src/cyberherd/delivery.rs`, `feeding_delivery.rs`; delivery and [feeding announcement](cyberherd-feeding-announcements-checkpoint-20261110.md) checkpoints. | Public note/overlay delivery is active. Observe remaining original-anchor and one-overlay scopes. Stream chat is intentionally disabled by the operator; preserve suppression of new and retained chat instead of reenabling it for acceptance. Preserve one feeding announcement per payment. |
| 11. Lifecycle messages | `src/cyberherd/lifecycle_tests.rs`; [lifecycle checkpoint](cyberherd-lifecycle-checkpoint-20261110.md). | Lifecycle notes are enabled and earlier native relay receipts are recorded. Qualify remaining installed pause/resume/evening and gateway-context/opt-out scopes. Stream-only invitations remain suppressed under the operator's chat opt-out. Unknown override never announces resumption. |
| 12. Interfaces and operations | [readonly site](cyberherd-live-readonly-site-20261110.md), [public self-read](cyberherd-self-read-public-live-20261111.md), [operator qualification](cyberherd-installed-operator-checkpoint-20261111.md), November 13 upgrade checkpoint. | Execute only approved maintenance; retain the completed explicit financial/public activation and complete remaining operator/runtime acceptance, recovery adoption and the full end-to-end audit. Keep browser keys in the extension/bunker. |

## Cross-cutting operator requirements

- Three user spots; no NIP-05 admission gate; America/Denver calendar.
- CyberHerd notes and overlay messages are enabled; CyberHerd stream chat is
  intentionally disabled, including retained chat retries. Do not reenable it
  to satisfy an older upstream conversation requirement.
- Zap payment/feed copy must mention the verified sender and actual settled
  amount, with profile links for Nostr and no public sender attribution for
  anonymous requests. Real external observation remains open.
- Maximum three physical feedings and one feeding announcement per payment.
  The erroneous original backlog retirement is complete; do not repeat it or
  erase later donors' credits. See the [repair contract](../deployment/per-payment-feeder-cap.md).
- Payment announcements require ten credited sats. Information/weather are
  overlay-only; paused state remains prominent without scrolling-message prefixes.
- Rewards use actual confirmed feedings. Selected gross limits include actual
  fees deducted from the user share: 10%, 100 sats/feed, 1,000 sats/rolling day,
  100 sats delivered/payment, 100 new signing requests/hour. These settings do
  not authorize execution. Unaffordable shares remain with operations.
- Preserve source settlement identity, exact signed bytes, feed UUIDs, private
  credential boundaries, existing gateway authority and operator-owned override.
- A restored snapshot may omit external side effects. The
  [recovery contract](../deployment/cyberherd-state-maintenance.md) still requires
  complete provider/feeder reconciliation and reviewed adoption. Neither a wallet
  balance nor an empty list of locally known payments proves interval coverage.

Do not mark the objective complete by checking only the current migration or CI.
For each row, retain actual evidence for its full scope and keep unsupported or
unobserved behavior open. Production changes remain separate operator steps.
