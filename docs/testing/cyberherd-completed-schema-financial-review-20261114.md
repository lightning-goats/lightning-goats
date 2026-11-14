# Financial preparation after schema maintenance — source checkpoint

The approved [live inclusive-fee upgrade](cyberherd-inclusive-upgrade-live-20261114.md)
completed, but the earlier financial maintenance adapter accepted only reviews
marked `program_upgrade_only=true`. Reusing that guard would reject the host's
actual completed schema review. The former installed fixture hid the integration
gap by supplying its earlier same-build program-only review.

Source `595b262` adds a distinct completed-schema acceptance path. It retains the
review digest, six installed program hashes, original identities/owner/epoch/
provider and completed zero-budget resumption checks. It additionally requires
accepted migration flags and before/after manifest hashes, then uses the current
hash-matched native verifier to recheck the retained old source, migrated bundle
and four-journal comparison. Results must match the retained original receipts.
It never executes historical review code, opens live journals, migrates, changes
configuration or starts services. Unknown/ambiguous transaction types remain refused.

The disposable financial activation scenario now consumes the completed schema
review rather than the earlier program-only review. Local regression checks pass
for successful evidence binding, changed manifest refusal before native execution,
changed acceptance flags and mismatched source/comparison identities. Native calls
are mocked in those local checks; nine protected-root cases are explicitly skipped
outside root CI. The focused operator-edge suite passes five tests.

At this checkpoint GitHub reports the PR open and mergeable, but zero check suites
and zero Actions runs for the exact pushed source. No previous run qualifies this
new integration. Installed/native CI acceptance and a separately qualified tool
generation remain required before actual-host financial preparation. No live
financial candidate, payout activation or provider execution was performed.
