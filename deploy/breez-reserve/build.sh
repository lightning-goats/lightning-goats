#!/usr/bin/env bash
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
[[ $# -eq 2 ]] || { echo 'Usage: build.sh NEW_BUILD_DIRECTORY PYTHON_3_11' >&2; exit 2; }
mkdir -- "$1"
build=$(cd -- "$1" && pwd)
python=$(realpath -- "$2")
python_prefix=$(dirname -- "$(dirname -- "$python")")
[[ -f "$python_prefix/BUILD" && -f "$python_prefix/lib/python3.11/os.py" ]] || {
    echo "Supply Python 3.11 from a relocatable python-build-standalone distribution." >&2
    exit 2
}
[[ "$("$python" -c 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")')" == 3.11 ]]
spark_rev=5bb1ca5229cd4d6d4e8cf66dfe9d01224db6cd2d
nutshell_rev=3ab0137b0c6b214cecc2f883be606a668ae74e60
cargo_target=$(realpath -m -- "${CARGO_TARGET_DIR:-$build/target}")

git clone --quiet --depth 1 --branch 0.28.6 https://github.com/breez/spark-sdk.git "$build/spark"
git clone --quiet --depth 1 --branch 0.22.1 https://github.com/cashubtc/nutshell.git "$build/nutshell"
[[ "$(git -C "$build/spark" rev-parse HEAD)" == "$spark_rev" ]]
[[ "$(git -C "$build/nutshell" rev-parse HEAD)" == "$nutshell_rev" ]]
git -C "$build/spark" apply "$here/wallet/spark.patch" "$here/wallet/lock.patch"
git -C "$build/nutshell" apply "$here/mint/nutshell.patch" "$here/mint/runtime-deps.patch"
mkdir "$build/spark/crates/lg-reserve"
cp -a "$here/wallet/Cargo.toml" "$here/wallet/src" "$build/spark/crates/lg-reserve/"
install -m 0644 "$here/mint/adapter.py" "$build/nutshell/cashu/lightning/goats_reserve.py"
install -m 0644 "$here/mint/lg_mint.py" "$build/nutshell/lg_mint.py"

(
    cd "$build/spark"
    export CARGO_TARGET_DIR="$cargo_target"
    cargo fmt -p lightning-goats-reserve --check
    cargo clippy --locked -p lightning-goats-reserve --all-targets --all-features -- -D warnings
    cargo test --locked -p lightning-goats-reserve --all-features
    cargo build --locked --profile release-lnurl -p lightning-goats-reserve
)
(
    cd "$build/nutshell"
    export POETRY_VIRTUALENVS_IN_PROJECT=true
    export POETRY_VIRTUALENVS_PATH="$build/poetry-envs"
    poetry env use "$python"
    poetry install --only main --no-root --no-interaction
    PYTHONPATH="$build/nutshell" .venv/bin/python "$here/tests/mint_acceptance.py"
)

bundle=$build/bundle
mkdir -p "$bundle/bin" "$bundle/nutshell" "$bundle/site-packages" "$bundle/python"
install -m 0755 "$cargo_target/release-lnurl/lightning-goats-reserve" "$bundle/bin/"
cp -a "$build/nutshell/cashu" "$build/nutshell/lg_mint.py" "$bundle/nutshell/"
cp -a "$build/nutshell/.venv/lib/python3.11/site-packages/." "$bundle/site-packages/"
rm -rf -- "$bundle/site-packages/pip" "$bundle/site-packages"/pip-*.dist-info
# Use a relocatable Python distribution whose bin/ and lib/ are below one prefix.
cp -a "$python_prefix"/. "$bundle/python/"
find "$bundle" -type d -name __pycache__ -prune -exec rm -rf -- '{}' +
printf 'Breez Spark 0.28.6 %s\nNutshell 0.22.1 %s\n' "$spark_rev" "$nutshell_rev" > "$bundle/UPSTREAM.txt"
"$bundle/python/bin/python3.11" -c 'import sqlite3, sys; print(sys.version.split()[0], sqlite3.sqlite_version)' >> "$bundle/UPSTREAM.txt"
printf '%s\n' "$bundle"
