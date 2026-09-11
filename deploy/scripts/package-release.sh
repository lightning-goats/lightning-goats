#!/usr/bin/env bash
# Build a complete archive from reviewed, already-built binaries. No installation.
set -euo pipefail

BIN_DIR=$(realpath "${1:?usage: package-release.sh BIN_DIR NEW_OUTPUT_DIR VERSION}")
OUTPUT=$(realpath -m "${2:?missing output directory}")
VERSION=${3:?missing version}
[[ "$VERSION" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]] || { echo 'Invalid version' >&2; exit 1; }
REPO=$(cd -- "$(dirname -- "$0")/../.." && pwd)
if [[ -n $(git -C "$REPO" status --porcelain --untracked-files=normal) ]]; then
    echo 'Package only a clean source checkout; put output outside the checkout.' >&2
    exit 1
fi
SOURCE=$(git -C "$REPO" rev-parse HEAD)
EPOCH=$(git -C "$REPO" show -s --format=%ct HEAD)
ARCHIVE="lightning-goats-${VERSION}-x86_64-linux-gnu.tar.gz"
mkdir -- "$OUTPUT"
STAGE=$(mktemp -d)
trap 'rm -rf -- "$STAGE"' EXIT

for binary in lightning-goatsd lightning-goatsctl lightning-goats-gateway; do
    test -x "$BIN_DIR/$binary"
    install -m 0755 "$BIN_DIR/$binary" "$STAGE/$binary"
    install -m 0755 "$BIN_DIR/$binary" "$OUTPUT/$binary"
done
cp -R "$REPO/deploy" "$REPO/docs" "$REPO/web" "$STAGE/"
cp "$REPO/AGENTS.md" "$REPO/Cargo.lock" "$STAGE/"
{
    printf 'source_commit=%s\nversion=%s\ntarget=x86_64-unknown-linux-gnu\n' "$SOURCE" "$VERSION"
    rustc --version --verbose
    cargo --version
} > "$STAGE/BUILD-INFO.txt"
cp "$STAGE/BUILD-INFO.txt" "$OUTPUT/"
(
    cd "$STAGE"
    find . -type f ! -name SHA256SUMS -print0 | LC_ALL=C sort -z | xargs -0 sha256sum > SHA256SUMS
    tar --sort=name --mtime="@$EPOCH" --owner=0 --group=0 --numeric-owner -cf - . | gzip -n > "$OUTPUT/$ARCHIVE"
)
(
    cd "$OUTPUT"
    sha256sum lightning-goatsd lightning-goatsctl lightning-goats-gateway BUILD-INFO.txt "$ARCHIVE" > SHA256SUMS
)
printf '%s\n' "$OUTPUT/$ARCHIVE"
