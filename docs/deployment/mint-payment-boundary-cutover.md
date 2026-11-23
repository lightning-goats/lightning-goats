# Goats payment boundary cutover

## VPS claim worker

On 2026-11-23 at 16:45:52 UTC the VPS switched the inbound claim worker to the collector from main revision `d07a567` (PR #167, feature revision `a089d97`). Its previous entrypoint was `/opt/lightning-goats-project-collector/collector/worker.py`. The release directory is `/opt/lightning-goats-project-collector/releases/claims-a089d97`. The live database and pinned-key configuration paths come from the existing unit. This stage has no database migration.

1. Compare the installed programs with the release. Verify the Git blob IDs of `worker.py`, `claim_store.py`, `collect.py`, `project_quote.py` and `operator_proofs.py` against the merged revision. Install those five files in a root-owned revision directory. With the unit's Python runtime and module search path, run the staged worker with `--help` as the collector identity.
2. Retain the original executable path and install a service drop-in that resets `ExecStart` and selects the new worker. Keep the unit's existing arguments, identity, environment and filesystem controls. Run `systemctl daemon-reload` and restart only `lightning-goats-project-claims.service`.
3. Verify the effective executable path, service start time and restart count. Observe the public mint capability and match quote IDs, invoices, units and amounts against journal bindings without printing them. To confirm a payment, correlate the signed invoice hash, collected proofs and the application receipt. Count a feed credit only for its original eligible receipt.
4. To revert this stage, remove only the `60-nut29-a089d97.conf` drop-in, reload systemd, and restart the claims service. Resume from the current journal. A paid or issued quote can require the original output secrets even after invoice expiry.

The production mint advertised BOLT11 batch checks with a maximum of 1,000. A read-only status comparison returned two unpaid quotes and one issued quote, all bound to the expected journal records. An isolated mint with a simulated Lightning backend produced identical per-quote proofs using one batch status call and three individual status calls. The fixture covered a lost issuance reply and a collector restart.

## Legacy records to reconcile

The read-only VPS inventory on 2026-11-23 found 109 Strike invoice records (last issued 2026-11-21 17:05:39 UTC), 33 Strike settlement records, 109 scan entries and 90 completed webhook inbox entries. Invoice and settlement counts measure different events. An invoice's expiry and the absence of a webhook do not prove nonpayment. Before retiring the historical recovery path, check original invoice identities against authenticated provider records and replay late positive evidence through the existing uniqueness constraints.

Four Strike treasury intents for 1 sat each, issued on 2026-11-19, have closed delivery inspections. All four arrived in the isolated wallet journal. None has a prepared melt, bound melt request, wallet result, or recorded Strike deposit. Keep their identifiers and historical proof ownership records for reconciliation. Do not resend those invoices or transfer their amounts based on missing local evidence. The legacy payout provider journal had three submitted executions, each with a bound payment and a terminal completed record, and zero recorded conflicts.
