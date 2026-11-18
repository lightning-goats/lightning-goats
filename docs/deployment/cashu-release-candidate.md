# Cashu release candidate

Production remains on hold. Build the review candidate from a clean source checkout.
The archive contains the core daemon and CLI, feeder gateway, wallet daemon,
Cashu signer and wallet state tool. It includes the inactive wallet and signer
unit examples, shadow core configuration, nginx example, operator runbooks,
backup tools, the Cashu overlay and its assets, and the locked Rust dependency
notices. The bundled QR code library includes its MIT notice and source record.
Provider credentials, wallet state and private keys are provisioned separately
through the approved procedures.

```sh
cargo fetch --locked
cargo fetch --locked --manifest-path tools/cashu-compat/Cargo.toml
python3 deploy/scripts/package-cashu.py build /tmp/lg-cashu-review
(
  cd /tmp/lg-cashu-review
  sha256sum --check SHA256SUMS
)
python3 deploy/scripts/package-cashu.py verify /tmp/lg-cashu-review/*.tar.gz "$(git rev-parse HEAD)"
```

Use Rust and Cargo 1.88, a native x86-64 GNU/Linux host, GNU binutils, a C
compiler, Python 3.11 or later and the cached locked crates. The builder uses
`--release` by default through Cargo's release profile. Supply `--profile dev`
for a local build rehearsal. Both profiles compile offline after the cache is
populated. The deployment workflow assembles the release profile from the
selected Git checkout. Run the verifier against the independently selected
source commit and artifact checksum.

`BUILD-INFO.json` records the source tree, tool versions and executable hashes.
Each executable records Cargo packages whose archive members GNU ld selected,
Rust toolchain and system archive members, the ELF shared-library requirements
and the highest required numeric glibc symbol version. The archive selection
table may include objects whose sections the linker subsequently discarded.
Build-only and other locked packages retain notice coverage in the complete
offline export. The packaged Rust toolchain notices accompany the selected
standard library code. The host supplies system libraries. Use the existing
source-pinned procedures for external tools such as `nak` and `age`.

The builder runs `--help` on each trusted build and writes the archive with
ordered members, normalized modes and Git-commit timestamps. Archive assembly
is deterministic with the same build inputs. Independent compiled-binary
reproducibility across hosts remains unverified. The verifier reads the
archive directly and checks its contents, hashes, lock inventories, source
identity and ELF headers. Installed startup and live provider or feeder
acceptance follow their existing verification procedures.
