# Upstream notice supplements

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

Development export:

```sh
python3 deploy/scripts/export-cashu-notices.py \
  docs/testing/cashu-license-inventory-20261117.json \
  .artifacts/cashu-development-notices-20261117-v9 \
  --supplement docs/licenses/cashu-upstream-supplement.json --allow-incomplete
```

The output directory must not already exist. This checkpoint exports 462 package
entries with 15 still missing notice files. It remains `distribution_ready:false`;
`--allow-incomplete` is for development review only. SPDX expressions and notice
presence alone do not qualify a distributable linked artifact. Evidence:
[export result](../testing/cashu-license-authors-20261117.json).

The v9 [inventory correction](../testing/cashu-license-authors-20261117.json)
recognizes the full permission/copyright notice that `r-efi` ships in `AUTHORS`.
Both checksum-verified archives match their exact upstream Git blob. This does
not treat a contributor list alone as a license notice. The three archive
regressions and ten supplement checks pass, and strict export still refuses
the remaining 15 missing notices without creating output. The development
inventory's complete dependency coverage does not establish distribution readiness.

The Gloo archive records a workspace subdirectory URL under `rustwasm/gloo`.
Its public repository redirects to `ranile/gloo`; the exact recorded commit
remains available there. The provenance retains the original URL, retrieved URL
and Git blob ID. Repository metadata normalization accepts a workspace `tree/`
path while preserving the owner/repository; it rejects credentials, query
parameters, traversal and unrelated path forms. Ten validation tests pass, including the actual vendored-source check.
