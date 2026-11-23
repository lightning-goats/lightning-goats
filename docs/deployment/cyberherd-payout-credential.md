# CyberHerd payout custody

New payout installations use the Goats collector. Set `funding_source` to `retained_cashu` and configure the private collector payout socket. For a fresh journal, bind the `retained-cashu` provider scope to the core epoch and the selected custody identity fingerprint. Verify those values before initialization, which persists the binding. Keep the monetary limits and execution controls disabled until activation.

Historical journal scopes retain the original account and credential fingerprint across upgrades and recovery. Preserve original payment IDs and signed outcomes. Keep archived credentials protected until the operator has reconciled every original obligation. The receiver and ongoing payout workers use the mint-facing paths.

See the [payment boundary cutover runbook](mint-payment-boundary-cutover.md) for deployed revisions, original obligations and recovery.
