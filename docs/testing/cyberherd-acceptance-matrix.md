# CyberHerd full-port acceptance inventory

This inventory follows all twelve requirements in the
[implementation plan](../planning/cyberherd-implementation-plan.md). It is not a
completion declaration. A test file identifies coverage to inspect; its existence
alone does not prove a passing test or live acceptance. Historical live checkpoints
prove their recorded observations, not that today's service configuration is the
same. Current production review and qualification are recorded in the
[November 13 checkpoint](cyberherd-inclusive-upgrade-checkpoint-20261113.md).

The current staged review is an additive payout-schema upgrade with unchanged
configuration and disabled outgoing roles. It cannot complete the full port.

| Requirement | Source and recorded evidence | Remaining acceptance |
| --- | --- | --- |
| 1. Membership and arithmetic | `src/cyberherd/store.rs`, `shares.rs`; [live social intake](cyberherd-live-social-intake-20261110.md) records reaction/repost admission and restart retention. | Revalidate three-user capacity and current settings; exercise settled zap increase/displacement and rejection rules on the selected installed stage. NIP-05 remains unnecessary. |
| 2. Points and shares | `src/cyberherd/shares.rs`, `ingress_tests.rs`; social checkpoint records exact live scores/shares. | Check the complete once-per-cycle/pre-join/re-entry and largest-remainder cases in exact-source test results; correlate paid-member shares with authoritative settlement before enabling rewards. |
| 3. Durable cycles | `src/cyberherd/store.rs`, `operator_tests.rs`; transactional/restart fixtures are part of Rust qualification. | Verify selected installed Mountain-time reset/catch-up behavior and operator idempotency without resetting paid history. A membership-only restart does not prove every reset path. |
| 4. Targets and profiles | `src/cyberherd/targets.rs`, `tests/cyberherd_targets.rs`; public leaderboard checkpoint records authenticated target links. | Verify current-day note, stream versions/aliases, linked goal and stale/ended precedence on the selected runtime; retain signature/host-authority rules. |
| 5. Ingestion and pause recovery | `src/cyberherd/worker.rs`, `ingress_tests.rs`; live social checkpoint records bounded recent/historical recovery and no duplicate awards. | Verify current installed override/defer/refresh/replay/discard behavior, including discovery unavailable and active ordinary zap eligibility. |
| 6. Incoming NIP-57 | `tests/zap_receive.rs`; [receipt checkpoint](cyberherd-zap-receipt-checkpoint-20261110.md). | Complete externally observed zap settlement, exact description-hash binding, durable receipt and exactly-once membership consumption. Ordinary Lightning receipt is insufficient proof. |
| 7. Confirmed-feeding rewards | `src/cyberherd/rewards_tests.rs`, `reward_store.rs`; [funding](cyberherd-payout-funding-checkpoint-20261109.md) and [inclusive fees](cyberherd-inclusive-fees-checkpoint-20261111.md). | Adopt the upgraded journal, reconcile retained backlog and qualify real funding/recipient outcomes. Only confirmed physical UUIDs earn rewards; virtual credits and threshold messages do not. |
| 8. Isolated outgoing payments | `src/payout/worker_tests.rs`, `provider_workflow_tests.rs`, `cost_recovery_tests.rs`; [recipient qualification](cyberherd-recipient-checkpoint-20261110.md). | Real provider/recipient acceptance, inclusive fee deduction, retained original-ID reconciliation and external restore adoption remain open. No real spending is authorized by synthetic tests or selected financial settings. |
| 9. Messages and delivery | `src/cyberherd/delivery_tests.rs`, `result_tests.rs`; [member delivery](cyberherd-member-delivery-checkpoint-20261110.md). | Verify installed public membership/treat output, original frozen increments/footer, unchanged retries and template deletion/custom-edit behavior. No treat notice precedes authoritative payout settlement. |
| 10. Conversation destinations | `src/cyberherd/delivery.rs`, `feeding_delivery.rs`; delivery and [feeding announcement](cyberherd-feeding-announcements-checkpoint-20261110.md) checkpoints. | Observe correct member-note and active-stream branches, retained original anchors and one overlay copy after explicit publishing activation. Preserve one feeding announcement per payment. |
| 11. Lifecycle messages | `src/cyberherd/lifecycle_tests.rs`; [lifecycle checkpoint](cyberherd-lifecycle-checkpoint-20261110.md). | Qualify installed invitation/pause/resume/evening behavior, supported gateway context and template opt-out before any public lifecycle rollout. Unknown override never announces resumption. |
| 12. Interfaces and operations | [readonly site](cyberherd-live-readonly-site-20261110.md), [public self-read](cyberherd-self-read-public-live-20261111.md), [operator qualification](cyberherd-installed-operator-checkpoint-20261111.md), November 13 upgrade checkpoint. | Execute only approved maintenance; complete actual operator/runtime acceptance, recovery adoption, explicit financial/public activation and full end-to-end audit. Keep browser keys in the extension/bunker. |

## Cross-cutting operator requirements

- Three user spots; no NIP-05 admission gate; America/Denver calendar.
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
