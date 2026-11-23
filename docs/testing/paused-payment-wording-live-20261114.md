# Natural paused-payment wording — 2026-11-14

The operator requested a more natural queue notice after examples repeated the
funded queue and announced an empty queue. Payment templates now thank the donor
first and include one frozen pause/progress description. Two representative
template renderings are:

> Thank you for 2100 sats! Feeding is paused. We have 2 feedings lined up for when feeding resumes. Another 221 sats are saved toward the next feeding.

> Thank you for 50 sats! Feeding is paused. Your sats are saved, with 879 more needed to fund the next feeding.

Normal legacy template/fact selection and optional goat naming remain intact.
Custom templates omitting the progress placeholder retain a pause notice. Queue
counts still use the original frozen context, including an earlier feeding awaiting
confirmation; grammar no longer uses `feeding(s)`. Overlay text retains its
separate status-banner behavior. The ten-sat announcement floor, financial state,
feeder dispatch and payment/feeding deduplication are unchanged. Already signed
messages are never rewritten or signed again to change their wording.

## Qualification

Source `d03cfc5fcfde2f5938d49aa8011189710fbefcc0` and tested merge
`a182bed301776d8f831a1b7bcdca48f4a4ba23f7` have identical tree
`ba185865da902495f555fafcb4a52e900e00f853`. Runtime changes since the installed
build are confined to presentation; other source changes are tests/documentation.
No migration, dependency or financial protocol changed.

[Rust CI](https://github.com/lightning-goats/lightning-goats/actions/runs/43600891318)
passed formatting, strict locked Clippy and all-feature tests. The log explicitly
shows the new four-case paused-payment regression, unchanged overlay/audience
checks and frozen-context restart/no-debit regression passing. The library suite
had 419 passing tests, zero failures and six ignored; integration and separate
mandatory root fixtures also passed.
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/43600891242),
[NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/43600891346)
and all home checks passed. [Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/43600891243)
passed all five jobs, including systemd job `132467087676`.

Release artifact `14116790656` ZIP SHA256:
`4818ce9115d69ad20095a421f612ead8aefdc186825d4b7454edfd98185ff0a1`.
Outer/internal checksums, archive source identity and the complete payload were
verified before executable staging. Archive SHA256:
`cadc7b3d8709eea753478b44febf39183d2e2172b212c24c372fc6596091e09d`.

## Actual core-only maintenance

The earlier zero-budget installation coordinators are inappropriate for the now
active payout stage. This update instead used the existing hash-authenticated
file-transaction, service-mask/restore and native backup utilities for a narrow
one-executable maintenance operation. It captured all four active identities,
units/drop-ins, configurations and six executable fingerprints, retaining the
original core, verifier and custom core unit privately. The sealed one-file
preparation SHA256 is
`56d974dedb16e05b9cf3e9aa62c4abfde6062caf232ba38c6db7fe3715ed028c`.

All four writers were stopped/masked, and absence of runtime-UID processes was
verified. A fresh native four-journal snapshot and verification passed. The
recoverable file primitive replaced only the core executable, preserving its
root ownership, mode and attributes. Guards proved all live main/WAL images,
all four configurations and other executables unchanged. Applied receipt SHA256:
`47d0e25e69b76ca4c0c921d414c0de5d27a64b75eccedb948a1d31b60c3d257f`.

Separate resumption reverified the accepted backup and unchanged live images
before restoring the exact original unit and existing enablement. It started core,
signer, membership and payout, checking actual running executable identities and
local health. All four were active/running/enabled with zero automatic restarts.
Installed core SHA256:
`40cdf2ce070cb69e5094e7bed9f0c1a98d94e4b2b8d49c4566b4826480edbbbc`.
No other program, credential, configuration, schema or financial ceiling changed.
No database was restored, no signed outbox was edited and no test payment, credit,
publication or feeder actuation was submitted by this maintenance.

Post-resumption status retained 2,221 sats, two feedings due, remainder 221,
override ON, reachable gateway and no unresolved feeding. The approved financial
activation remains enabled. Renderer regressions establish the new wording;
no claim is made that a new real payment/post was observed after this deployment.
