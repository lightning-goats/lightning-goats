#!/usr/bin/env bash
# Review archive only. Never installs files, creates users, or starts services.
set -euo pipefail
LG_BIN_DIR=$(realpath "${1:?usage: package-cyberherd.sh BIN_DIR NEW_OUTPUT_DIR VERSION}")
LG_OUTPUT=$(realpath -m "${2:?missing output directory}")
LG_VERSION=${3:?missing version}
[[ "$LG_VERSION" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]] || { echo 'Invalid version' >&2; exit 1; }
LG_REPO=$(cd -- "$(dirname -- "$0")/../.." && pwd)
if [[ -n $(git -C "$LG_REPO" status --porcelain --untracked-files=normal) ]]; then
    echo 'Package a clean source checkout; output must be outside it.' >&2
    exit 1
fi
LG_SOURCE=$(git -C "$LG_REPO" rev-parse HEAD)
LG_EPOCH=$(git -C "$LG_REPO" show -s --format=%ct HEAD)
mkdir -- "$LG_OUTPUT"
LG_STAGE=$(mktemp -d)
trap 'rm -rf -- "$LG_STAGE"' EXIT
for LG_BINARY in cyberherdd cyberherd-payoutd cyberherd-signerd; do
    test -x "$LG_BIN_DIR/$LG_BINARY"
    "$LG_BIN_DIR/$LG_BINARY" --help >/dev/null
    install -m 0755 "$LG_BIN_DIR/$LG_BINARY" "$LG_STAGE/$LG_BINARY"
done
cp -R "$LG_REPO/cyberherd-migrations" "$LG_REPO/payout-migrations" "$LG_REPO/signer-migrations" "$LG_STAGE/"
mkdir -p "$LG_STAGE/deploy/systemd"
cp -R "$LG_REPO/deploy/cyberherd" "$LG_STAGE/deploy/"
for LG_UNIT in cyberherd cyberherd-payout cyberherd-signer; do
    cp "$LG_REPO/deploy/systemd/$LG_UNIT.service.example" "$LG_STAGE/deploy/systemd/"
done
cp "$LG_REPO/Cargo.lock" "$LG_REPO/Cargo.toml" "$LG_STAGE/"
cp -R "$LG_REPO/vendor" "$LG_STAGE/"
mkdir -p "$LG_STAGE/deploy/scripts"
cp "$LG_REPO/deploy/scripts/verify-sqlite-vendor.py" "$LG_STAGE/deploy/scripts/"
python3 "$LG_REPO/deploy/scripts/verify-sqlite-vendor.py" "$LG_STAGE"
cp "$LG_REPO/docs/architecture/cyberherd-payout-runtime.md" "$LG_STAGE/README.md"
printf 'source_commit=%s\nversion=%s\nactivation=none\n' "$LG_SOURCE" "$LG_VERSION" > "$LG_STAGE/BUILD-INFO.txt"
(
    cd "$LG_STAGE"
    find . -type f ! -name SHA256SUMS -print0 | LC_ALL=C sort -z | xargs -0 sha256sum > SHA256SUMS
    tar --sort=name --mtime="@$LG_EPOCH" --owner=0 --group=0 --numeric-owner -cf - . | gzip -n > "$LG_OUTPUT/cyberherd-$LG_VERSION-x86_64-linux-gnu.tar.gz"
)
(
    cd "$LG_OUTPUT"
    sha256sum "cyberherd-$LG_VERSION-x86_64-linux-gnu.tar.gz" > SHA256SUMS
)
printf '%s\n' "$LG_OUTPUT/cyberherd-$LG_VERSION-x86_64-linux-gnu.tar.gz"
