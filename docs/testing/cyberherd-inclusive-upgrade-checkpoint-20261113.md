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
