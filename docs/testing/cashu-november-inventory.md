# Nut November source inventory — development record

This is preparation for the scoped [Cashu plan](../planning/cashu-nutnovember-implementation-plan.md),
not a submitted entry or release. Public tag/PR selection, final source review,
independent-client acceptance, demo, and operator-approved live qualification
remain unfinished. Recheck the official event instructions before submission.

## References

The selected pre-November reference is
`55ed163fed6742fc79d7c5c34977bb69ea90a439`, tree
`c4859fa65016fcb7dad50582bfc23c6672bda99a`, committed
2026-10-06T08:49:40-06:00. It is the last first-parent commit before
2026-11-01T00:00:00Z in the Cashu starting revision's history. This selection
uses repository commit metadata; it does not establish an external publication
timestamp or event eligibility by itself.

The Cashu workstream starts from
`54c053b1ae655c829fc6bed96ad59b22b409c082` (2026-11-15T09:06:31-07:00),
on branch `feat/cashu-nutnovember-20261115`. The active checkout is an uncommitted
implementation. No release tag or final immutable Cashu source revision exists
yet. The starting revision already contains substantial November Lightning,
CyberHerd, website and deployment work; those inherited changes must not be
presented as Cashu additions.

Reproduce the reference selection:

```sh
git rev-list --first-parent -1 --before=2026-11-01T00:00:00Z 54c053b1ae655c829fc6bed96ad59b22b409c082
git merge-base --is-ancestor 55ed163fed6742fc79d7c5c34977bb69ea90a439 54c053b1ae655c829fc6bed96ad59b22b409c082
git show -s --format='%H%n%T%n%cI%n%s' 55ed163fed6742fc79d7c5c34977bb69ea90a439
```

## Cashu additions to describe in the eventual package

| Addition after the Cashu starting revision | Evidence and present limitation |
| --- | --- |
| Separate pinned CDK wallet dependency/storage boundary | [Wallet boundary](../architecture/cashu-wallet-boundary.md) and [runtime instructions](../deployment/cashu-wallet-development.md); installed custody qualification pending. |
| Common ordinary-Cashu and NIP-61 validation/acquisition journal | Wallet regression evidence linked from [implementation status](../implementation-status.md); independent sender applications remain unqualified. |
| Authenticated receipt import into shared herd credit | Retained receipt and cross-UID fixtures linked from implementation status; no acquired ecash is described as Strike settlement. |
| Separate financial feed plans, purpose-bound Strike settlement and sweeps | [Sweep checkpoint](cashu-sweeps-20261116.json), [combined settlement](cashu-combined-settlement-20261116.json) and [lost-reply composition](cashu-combined-lost-reply-20261116.json); combined nonzero-fee/change case remains open. |
| Reusable request preparation and requested two-QR layout | [Native request CLI](cashu-request-cli-native-20261116.json), [44 browser tests](cashu-overlay-collisions-20261116.json) and [long request decode](cashu-overlay-long-request-20261116.json); phone scans and real wallet compatibility pending. |
| Public descriptor metadata selection and private read-only lookup | [Configuration checkpoint](cashu-public-request-config-20261116.json); subsequent IPC/HTTP/snapshot full gates running, positive cross-UID composition pending. |
| Inactive service and edge examples | [Unit syntax](cashu-inactive-units-20261116.json) and [nginx routing/log bounds](cashu-nginx-20261116.json); installed permissions, external WAF/CDN and provider acceptance pending. |

Do not include spendable tokens, seeds, private receiving keys, credentials,
status capabilities, production journal backups or real donor data in the public
package or recording. Synthetic request fixtures use `.example` destinations and
must never be funded.

## Required completion evidence still to collect

The final package needs an immutable public source revision/tag, full-source
checks and security review, pinned setup/compatibility instructions, and a clear
license inventory for the wallet package and distributed vendored dependencies.
The core and shared protocol packages declare MIT. The integrated wallet
manifest now also declares MIT, matching the repository license metadata.
The final release inventory still needs to retain all upstream vendor license
files and identify the exact distributed dependency artifacts.

The receiving demo must show distinct ordinary Cashu and nutzap contributions,
unchanged native Lightning, one shared progress display, actual constrained
settlement behavior, and replay causing no second contribution. Existing isolated
fixtures can supply honestly labeled synthetic financial evidence. A physical
feeding segment requires operator approval and actual observation; synthetic
ledger confirmation must be labeled as such. Do not submit a mock as live proof.

Coordinated backup/restore and rollback fences, protected operational commands,
private custody/age/lag health, positive end-to-end transport, independent clients,
external WAF/CORS compatibility, installed signer isolation and the selected live
mint/limit/key policy remain part of the full plan. Cashu-derived rewards remain
disabled and unqualified. Production activation and real-provider acceptance
remain operator-controlled steps. This inventory does not replace or narrow any
requirement in the implementation plan.

## Locked dependency license inventory — 2026-11-16

`deploy/scripts/inventory-cashu-licenses.py` inventories the union of the core and
isolated wallet lockfiles without fetching or building. Registry manifests and
license/notice file hashes come from cached crate archives whose SHA256 matches
Cargo.lock. Local packages retain their actual manifest hashes and top-level
license-file hashes, including the pinned SQLite/CDK adapters. The generated
[candidate inventory](cashu-license-inventory-20261116.json) names each graph and
keeps missing archives/licenses explicit; `--require-complete` refuses unresolved
entries. Optional and target-only packages are included and must not be described
as all linked or distributed.

This is package/declaration evidence, not a claim that a final release has been
assembled with every required notice. Retain the original upstream license files
in packaged vendor sources and qualify the final distribution against its exact
selected source and contents. No tagged release or submission is claimed.

The license scanner now includes files below license/notice directories (for example LICENSES/Apache-2.0.txt), not only files whose basename begins with LICENSE. The refreshed 462-entry inventory lists notice_files_missing separately and explicitly sets distribution_notices_complete=false. A resolved SPDX declaration and verified archive checksum do not establish that redistribution notices have been assembled. Missing upstream notice files require source-pinned provenance before packaging.

Archive inventory now retains normalized repository metadata and .cargo_vcs_info.json when present, bounded and read from the checksum-verified archive. Missing VCS metadata remains null: a repository URL alone does not pin the source of a missing notice. No upstream license text has been fetched or substituted by this step.

`deploy/scripts/export-cashu-notices.py` now exports inventoried text files from verified archives or local manifests, verifies every notice hash, and deduplicates identical content. The development export has 462 package entries and 239 unique verified texts, with 34 missing-file entries retained. Default invocation refuses incomplete input; the explicitly incomplete artifact is marked distribution_ready=false. See cashu-development-notices-20261116.json. Final source provenance for missing notices and a selected-distribution inventory remain required.

The [2026-11-16 official rules check](cashu-event-rules-check-20261116.md) reconfirms the deadline and November-change disclosure requirement. Full submission-form fields and final submission behavior remain unverified. No submission was made.

Added the missing top-level MIT text consistent with all four project package declarations. Their inventory entries now reference that project text; vendor entries retain their own provenance. The v3 development export matches the refreshed inventory hash, verifies every text, and leaves 30 missing upstream notice entries. Earlier exports remain tied to their original inventory hashes and are historical. Distribution readiness remains false.
