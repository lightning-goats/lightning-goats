#!/usr/bin/env bash
# Disposable CI build data only; never part of a production release archive.
set -euo pipefail
if [[ ${GITHUB_ACTIONS:-} != true || ${RUNNER_OS:-} != Linux || $EUID == 0 || $# != 1 ]]; then
  echo 'requires an unprivileged disposable Linux CI build and a new output directory' >&2
  exit 1
fi
legacy_source=be6ad767829b7d2693b4f1030aa3952f8b7d2555
output=$1
mkdir -m 0700 -- "$output"
mkdir -m 0700 -- "$output/source"
git fetch --no-tags --depth=1 origin "$legacy_source"
[[ $(git rev-parse FETCH_HEAD) == "$legacy_source" ]]
git archive "$legacy_source" | tar -x -C "$output/source"
# Separate target directory: an older build must never replace current release
# binaries that subsequent packaging steps select.
cargo build --manifest-path "$output/source/Cargo.toml" --target-dir "$output/target" \
  --release --locked --bin cyberherd-statectl
install -m 0755 "$output/target/release/cyberherd-statectl" \
  "$output/TEST-ONLY-schema8-statectl"
printf '%s\n' "$legacy_source" > "$output/SOURCE-COMMIT"
(cd "$output" && sha256sum TEST-ONLY-schema8-statectl SOURCE-COMMIT > SHA256SUMS)
