# Inclusive-fee upgrade qualification — 2026-11-13

The full CyberHerd port remains incomplete. This checkpoint qualifies an offline
schema-upgrade transaction; it does not authorize production maintenance or payouts.

## Immutable source and evidence

Reviewed branch source: `a6d6a8bd650cf5a89ca632bd60716d9390663609`.
CI-built PR111 merge: `ae10df666741c9160ada3b54a160435dc36f2b25`.
The merge object was fetched and its tree compared with the branch: no differences.
Artifacts identify the merge commit, which must be used for artifact verification.

All ten workflows passed. In particular:

- [Rust](https://github.com/lightning-goats/lightning-goats/actions/runs/43519507733):
  formatting, strict locked Clippy, full tests and seven mandatory actual-root
  native fixtures, including `ROOT_INCLUSIVE_FEE_UPGRADE_FIXTURE_VERIFIED`.
- [Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/43519507711):
  website, nginx, 167 actual-root Python cases, package assembly, genuine pinned
  older native verifier and the installed systemd scenarios.
- [Security](https://github.com/lightning-goats/lightning-goats/actions/runs/43519507797):
  the unchanged dependency/security gate passed.

The installed scenario reached the dedicated inclusive-fee apply/resume calls.
Its assertions require the exact seven-to-ten migration-history transition,
four-journal native comparisons, original configuration retention, core/member
resumption, outgoing roles disabled and zero gateway POSTs. Later synthetic
financial activation is separately identified as synthetic, with real provider
execution and external recovery adoption explicitly unqualified. The retained
summary does not separately export `INCLUSIVE-FEE-UPGRADE.json`; the successful
scenario's executed assertions are the evidence for this upgrade, not that absent
standalone artifact.

Downloaded core and review archives passed their checksum manifests, source and
payload verification. The core smoke test also passed all three help commands.
Archive checks and extraction used disk-backed scratch, not the nearly full `/tmp`.

| Artifact | SHA256 |
| --- | --- |
| Core release archive | `8ca90de62133b2bcb7ee2a9a1c5cbdcf7e56af39b16d1782e3587a090711dcb0` |
| CyberHerd review archive | `8d62689fff22fa6d7352a6808732ad1b948e76bfda0de94e38a69af297ff5237` |
| Exported tool manifest | `0aa603c19d0fcbb3efd96e73043f216bcd250526dbb5af171d8eaf27fd0a89a1` |
| Exported launcher | `528546d564557d8e9acbd3785345c0f4d986b7d6aecaa454b9da068e6210e74a` |

## Next production boundary

Preparation needs a new root-owned tool generation and a private review directory.
It copies/validates reviewed programs and configurations, invokes offline native
configuration checks under existing runtime identities, and seals a fresh baseline.
It does not stop/start services, open application journals, migrate state, load
credentials or enable payouts. A later apply/resume requires a separately reviewed
preparation digest and explicit maintenance approval.

Automatic approval review rejected the attempted tool installation into
`/usr/local/libexec/lightning-goats-inclusive-ae10df6`, citing the production hold
and lack of authorization for that persistent privileged filesystem change. The
command did not execute. Exported tools and archives remain staged only in the
workspace. No production tool installation, native journal migration, service
operation or financial activation was performed in this qualification session.

The next requested approval is only to install that isolated tool generation and
prepare the review using these exact artifacts. It does not authorize apply,
resumption, provider execution, publication or physical feeding. Existing
production programs/configuration remain the baseline until a later approved
maintenance transaction.

## Retained-summary follow-up

The follow-up source now places the dedicated `installed_inclusive_fee_upgrade`
result in `CYBERHERD-INACTIVE-SYSTEMD.json`, in addition to its fixture-local file.
It records observed migration-history counts, the pinned legacy verifier source,
native comparison/resumption acknowledgment, unchanged configurations and zero
fixture gateway POSTs. Synthetic/live-host/provider/recovery limits remain explicit.
This closes a reporting gap; it does not retroactively add an artifact to the
qualified run above. The revised retained summary requires its own installed CI
execution before being cited as observed evidence.

## Approved staging completed

The operator subsequently approved isolated tool installation and preparation.
The root-owned installed launcher and manifest match the exact digests above.
The authenticated `--inclusive-fee-upgrade prepare` command succeeded against the
qualified merge artifacts and a fresh live installation baseline.

Sealed review: `/var/lib/lightning-goats-upgrades/inclusive-fee-ae10df6-review`.
`PREPARED.json` SHA256:
`41a7c373e1dbc43754a1a3b30715383c47d0a0fd33a2d2308b1aebbf86bbb3c9`.

Independent readback confirms the dedicated schema transaction type, six selected
program hashes, and three accepted native configuration checks under the original
runtime UIDs in isolated networks. Journals opened, credentials loaded,
configurations changed, services changed and activation authority are all false.
Core and membership remain active; payout and signer remain masked.

The next requested maintenance scope is:

1. Recheck this sealed baseline and hold all four writers during a brief outage.
2. Take and verify a fresh native four-journal backup with both appropriate verifiers.
3. Migrate a separate payout copy under its existing UID with no network, verify
   all four journals, and preserve the original backup and paid/signed bytes.
4. Replace the six qualified programs, run the same offline migration on the
   existing payout journal, and require a fresh snapshot and strict comparison.
5. Resume only the existing core and membership services after the separate
   resumption fences pass. Keep original configuration, zero financial/signing
   budgets, payout/signer masks and household override unchanged.

This is a schema upgrade, not a program-only replacement. Failure after migration
stays held for inspection; no automatic stale-state restore or old-binary rollback
is authorized. The staging approval does not approve these maintenance steps.
Live provider execution, financial activation and external recovery adoption remain
outside this proposed window.
