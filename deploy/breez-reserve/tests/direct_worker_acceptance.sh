#!/usr/bin/env bash
# Run from a prepared Nutshell environment. Argument is its Python executable.
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo=$(realpath "$here/../../..")
python=$(realpath -s "$1")
messages=$(mktemp)
trap 'rm -f -- "$messages"' EXIT
cargo test --manifest-path "$repo/Cargo.toml" --lib --locked --all-features --no-run --message-format=json > "$messages"
binary=$("$python" - "$messages" <<'PY'
import json
import sys

with open(sys.argv[1]) as messages:
    binaries = [
        message["executable"] for line in messages
        if (message := json.loads(line)).get("reason") == "compiler-artifact"
        and message.get("executable") and message["target"]["kind"] == ["lib"]
    ]
binary, = binaries
print(binary)
PY
)
LG_DIRECT_WORKER_TEST="$binary" "$python" "$here/direct_operator_acceptance.py"
