# Proposed financial-candidate staging — 2026-11-14

This is a review-only proposal, not approval or financial activation. The schema
upgrade is already [complete](../testing/cyberherd-inclusive-upgrade-live-20261114.md).
The proposed candidate uses the operator's existing selected financial settings.

## Independently verified inputs

Qualified source: `ae10df666741c9160ada3b54a160435dc36f2b25`.
The existing installed isolated tools and launcher remain authenticated by their
[recorded manifest and launcher hashes](../testing/cyberherd-inclusive-upgrade-checkpoint-20261113.md).
The configuration preparer in that generation supports the inclusive-fee mode.

The separately built native checker comes from qualified Actions artifact
`14081999241`, `cyberherd-test-only-ae10df666741c9160ada3b54a160435dc36f2b25`.
Its downloaded ZIP SHA256 matches GitHub's recorded digest:
`884fd0dbbe0a78e335f1665bd793d99cb1445638dc2f051baa636e044e1048a6`.
The selected checker also matches the nested checksum manifest:
`7dc3903859dd79f9483ebfabfd0578e9f5c3df3c5fed0ef2f0a37d561f0a645f`
(4,277,232 bytes). Only this checker was extracted; none of the other test-only
executables will be installed by this proposal.

## Requested staging scope

1. Install that exact checker root-owned, mode `0755`, under a new isolated
   `/usr/local/libexec/lightning-goats-financial-checker-ae10df6/` directory.
2. Invoke the existing authenticated `--cyberherd-financial-stage` preparer into
   the new private review directory
   `/var/lib/lightning-goats-upgrades/financial-inclusive-ae10df6-candidate`.
3. Supply explicit selected limits: member 10%, gross 100 sats/feed and 1,000
   sats/rolling day, fixed fee field zero, maximum net 100 sats/payment,
   100 new requests/hour, `fee-mode=deduct_from_share`.
4. Require the actual stored allocation to match, unchanged original files/PIDs,
   exact four-role configuration bindings and positive native configuration-only
   validation under the original core UID in an isolated network namespace.
5. Retain the sealed review for independent inspection. Do not apply it.

Preparation opens no application journal, loads no credential, sends no provider
request, and changes no live configuration, systemd state or feeder control. The
rendered candidate contains proposed process flags and limits only as private
review data. Payout/signer remain masked and existing financial limits stay zero.
First financial installation and activation require separate qualification,
provider/recipient/recovery acceptance, fresh backup and explicit approval.

Automatic approval review rejected the attempted new checker installation because
it is a persistent production executable addition not covered by the earlier
maintenance approval. The command did not run. Checker bytes remain workspace
review data. Approval is requested only for the staging scope above.

The separate financial-maintenance adapter fix that consumes the completed schema
review is [under qualification](../testing/cyberherd-completed-schema-financial-review-20261114.md).
It does not need to be installed or executed merely to prepare this candidate.

The operator subsequently approved this scope. The [completed actual-host staging](../testing/cyberherd-financial-candidate-live-staging-20261114.md) records the
verified checker, accepted candidate and unchanged service/mask state. Historical
reward disposition remains unapplied and financial activation remains unauthorized.
