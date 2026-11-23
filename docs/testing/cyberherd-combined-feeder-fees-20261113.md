# Combined feeder and inclusive-fee source

The resumed implementation branch integrates the deployed three-feeding cap,
per-payment announcement deduplication, source-bound credit retirement and its
root-owner corrections into the inclusive-fee CyberHerd implementation.
Combined source: `b2a3b8f5b6af38cbf57cca8c390cbc26ea314986`.
The upstream explanation was refreshed on November 13; its SHA256 remains
`e56c6793ed0687e4fcac7df54dac48a375673308dc65e988e0dcd987af46e67a`.
Both the fee policy and subsequent feeder corrections remain operator overrides.

Formatting and diff checks pass. Seven inclusive-fee schema tests pass against
this tree. These tests cover the payout persistence constraints; they do not
prove composed worker behavior or live provider acceptance. Combined native CI
43507345884 passed the full Rust suite and mandatory root fixtures. Deployment run 43507345867 and
Security run 43507345730 are independent gates; their results must be inspected
before using a combined artifact. Earlier green source runs do not qualify this
new combined source automatically.

No production change was made during this integration. Next steps are to inspect
these exact running jobs, resolve integration failures, then qualify an immutable
combined artifact against the current installed schemas. The previously reviewed
same-schema program transaction must not implicitly migrate a payout journal.
Live provider/recipient acceptance, recovery adoption, public membership/lifecycle
rollout and the complete twelve-part acceptance audit remain open.

The required additive payout upgrade is specified in
[its contract](../deployment/cyberherd-inclusive-fee-upgrade.md). It explicitly
preserves the version-7 gap and keeps the same-schema maintenance guard intact.
The native source verifier/comparator and stopped-writer executor for this payout
upgrade remain to be implemented and qualified.
