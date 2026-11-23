# Cashu dependency notices

The license inventory records the contents of checksum-verified crate archives.
Some upstream workspaces omit root license files from those archives. The
supplement supplies exact upstream files without rewriting archive evidence.

`cashu-upstream-supplement.json` binds twelve registry packages to the immutable
upstream commit recorded in each archive's `.cargo_vcs_info.json`. Each notice
also records its upstream URL, Git blob SHA-1 and local SHA-256. The exporter
checks the inventory hash, package checksum, upstream repository/commit, canonical
repository path resolution and both file hashes before using these files. There
are no network downloads during export.

A thirteenth binding explicitly covers the vendored local `cdk` package. Before
accepting its original archive's upstream commit, the exporter runs the existing
vendor verifier: both original archives, complete source inventories, exact
allowed patches and locked dependency bindings must match. It also checks the
local CDK manifest against the license inventory. CDK retains `source: local`
in the exported index. Other local packages gain no implicit upstream authority;
changed manifests and unsupported local identities refuse.

Ten published Rust Bitcoin archives omit VCS metadata and the root CC0 text.
The supplement binds them to their signed release tags. Preparation checked
GitHub's tag verification results, recomputed the Git tree hashes against the
tagged commits, and matched every source member and original Cargo manifest to
the corresponding Git blob. That includes the shared workspace files Cargo
copied into some packages. The exporter rechecks those archive member hashes
against the reviewed record and checks that the notice bytes match the pinned
Git blob. Git tag verification is part of preparation. Export runs offline.
[Rust Bitcoin release evidence](../testing/cashu-rust-bitcoin-release-notices-20261117.json)

The three remaining LDK archives were compared with official workspace release
trees. All source members in `lightning-invoice@0.34.1` and
`possiblyrandom@0.2.1` matched. For `lightning@0.2.8`, 136 source members matched
and the archive's original Cargo manifest omits one three-line Git dependency
stanza. Both tagged tree hashes were recomputed. The recorded GitHub tag
signature verification is false for these releases. Their pinned MIT and
Apache notice blobs match the exported bytes.

The locked archives for `bitcoin-payment-instructions@0.7.1` and
`hex_lit@0.1.1` declare licenses and omit notice files. The export uses the
Apache-2.0 option declared by the former and the MITNFA text declared by the
latter, from the pinned SPDX catalog. The exporter matches the declaration
against the checksum-verified Cargo manifest and reports these texts under
`spdx_catalog_notices`. The archive inventory retains the publisher file
inventory and license declaration.
[Release notice evidence](../testing/cashu-release-notices-20261118.json)

Run the complete locked notice export offline:

```sh
python3 deploy/scripts/export-cashu-notices.py \
  docs/testing/cashu-license-inventory-20261117.json \
  .artifacts/cashu-notices-20261118 \
  --supplement docs/licenses/cashu-upstream-supplement.json
```

Choose a new output directory. The export covers all 465 locked package
entries. The report retains `distribution_ready:false` pending selection and
verification of the actual distribution artifact and its linked dependencies.

The v9 [inventory correction](../testing/cashu-license-authors-20261117.json)
recognizes the full permission/copyright notice that `r-efi` ships in `AUTHORS`.
Both checksum-verified archives match their exact upstream Git blob. This does
not treat a contributor list alone as a license notice. The three archive
regressions and ten supplement checks passed at that checkpoint. Strict export
then refused the remaining 15 missing notices without creating output.

The Gloo archive records a workspace subdirectory URL under `rustwasm/gloo`.
Its public repository redirects to `ranile/gloo`; the exact recorded commit
remains available there. The provenance retains the original URL, retrieved URL
and Git blob ID. Repository metadata normalization accepts a workspace `tree/`
path while preserving the owner/repository; it rejects credentials, query
parameters, traversal and unrelated path forms. Ten validation tests pass, including the actual vendored-source check.
