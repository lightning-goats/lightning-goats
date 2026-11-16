# Cashu crypto stdin helper

Stock nak v0.20.6 reads `encrypt` and `decrypt` payloads only from positional
arguments. The Cashu broker needs private history metadata to remain off process
argument lists. `bounded-stdin.patch` adds an explicit `--stdin` flag to those two
commands, accepting at most 1,024 UTF-8 bytes with one optional terminating LF.
It rejects combined positional/stdin input and includes bounded-input unit tests.
No cryptographic implementation, signing kind, publication behavior, or default
positional CLI behavior changes.

`manifest.json` pins the upstream commit/archive, exact patch, original changed
file, Go SDK version/archive and output version label. The upstream `go.mod` and
`go.sum` remain unchanged; Go commands use `-mod=readonly`, the checksum database,
one build job, a bounded Go heap target, and workspace-local caches. The output
is Linux amd64 with `CGO_ENABLED=1`, as required by upstream Linux LMDB support.
The host C compiler is recorded with the artifact; target-host compatibility
requires qualification.
It is a separate artifact, not a replacement for an installed production helper.

```sh
python3 tools/cashu-nak-stdin/build.py --build-directory /absolute/private/build-directory
```

The build directory must not already contain `nak-cashu-stdin`. The script
validates downloads, extracts fresh compiler/source trees, applies only the pinned
patch, runs its Go input-bound tests, builds with trimmed paths and no ambient VCS
metadata, and records the output SHA256/build metadata in `build-result.json`.
It neither installs a binary nor reads application credentials. Temporary compiler
and source trees are removed; verified archives and dependency/build caches remain
available for deliberate cleanup or subsequent qualification.

The pinned upstream main package has 22 existing nonconstant-format diagnostics
under this Go SDK. The build runs `go vet` on that package and requires its output
to match `upstream-vet-baseline.txt` exactly; any additional or changed diagnostic
fails the build. It then disables duplicate test-time vet while running the
bounded-input tests. This is a recorded upstream exception, not a clean vet
result. Unused upstream subpackages are outside this executable's vet scope.

The Rust adapter must use this qualified artifact's exact digest and a root-owned
installed path when eventually deployed. Its production path does not accept an
unsigned helper or bypass protected-parent checks. A stock helper with the correct
stock hash still lacks `--stdin` and must not be represented as qualified Cashu
crypto support.

Sources: [nak v0.20.6 commit](https://github.com/fiatjaf/nak/tree/fb21939f45538eff414068fa1bb6f1bea705f447),
[upstream crypto CLI](https://github.com/fiatjaf/nak/blob/fb21939f45538eff414068fa1bb6f1bea705f447/encrypt_decrypt.go),
and [official Go release checksums](https://go.dev/dl/?mode=json).
