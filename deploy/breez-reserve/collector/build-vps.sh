#!/usr/bin/env bash
# Assemble a relocatable VPS runtime from the pinned mint build environment.
set -euo pipefail
[[ $# == 3 ]] || { echo 'Usage: build-vps.sh PATCHED_NUTSHELL PYTHON_PREFIX NEW_OUTPUT_DIRECTORY' >&2; exit 2; }
mint=$(realpath -- "$1")
python=$(realpath -- "$2")
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
[[ $(git -C "$mint" rev-parse HEAD) == 3ab0137b0c6b214cecc2f883be606a668ae74e60 ]]
[[ -x "$python/bin/python3.11" && -d "$mint/.venv/lib/python3.11/site-packages" ]]
mkdir -- "$3"
out=$(realpath -- "$3")
mkdir "$out/collector" "$out/modules"
cp -a "$python" "$out/python"
cp -a "$mint/.venv/lib/python3.11/site-packages/." "$out/modules/"
cp -a "$mint/cashu" "$out/modules/"
install -m 0644 "$here"/{invoice_server,payout_server,claim_store,collect,worker,project_quote}.py "$out/collector/"
PYTHONNOUSERSITE=1 PYTHONDONTWRITEBYTECODE=1 PYTHONPATH="$out/modules:$out/collector" \
    "$out/python/bin/python3.11" -c 'import invoice_server, payout_server, worker'
