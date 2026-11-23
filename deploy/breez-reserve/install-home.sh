#!/usr/bin/env bash
set -euo pipefail
umask 077

if (( EUID != 0 )); then
    echo 'Run through sudo with the reviewed local bundle and its source revision.' >&2
    exit 2
fi
[[ $# -eq 2 && "$2" =~ ^[0-9a-f]{40}$ ]] || { echo 'Usage: install-home.sh BUNDLE SOURCE_COMMIT' >&2; exit 2; }
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
bundle=$(realpath -- "$1")
base=/opt/lightning-goats-reserve
release="$base/$2"
[[ ! -e "$release" && ! -L "$base/current" && ! -e "$base/current" ]]
[[ -x "$bundle/bin/lightning-goats-reserve" && -x "$bundle/python/bin/python3.11" ]]
[[ -f "$bundle/UPSTREAM.txt" && -f "$bundle/nutshell/lg_mint.py" ]]
for file in /etc/systemd/system/lightning-goats-{reserve,mint}@.service /etc/tmpfiles.d/lightning-goats-reserve.conf; do
    [[ ! -e "$file" && ! -L "$file" ]]
done
for identity in lg-reserve lg-mint; do
    if getent passwd "$identity" >/dev/null || getent group "$identity" >/dev/null; then
        echo "Existing identity requires review: $identity" >&2
        exit 1
    fi
done
for identity in lg-reserve lg-mint; do
    useradd --system --user-group --no-create-home --shell /usr/sbin/nologin "$identity"
done
install -d -m 0755 "$base" "$release" /etc/lightning-goats-reserve
cp -a "$bundle/." "$release/"
chown -hR root:root "$release"
chmod -R a+rX,go-w "$release"
ln -s "$release" "$base/current"
install -m 0644 "$here/lightning-goats-reserve@.service" "$here/lightning-goats-mint@.service" /etc/systemd/system/
install -D -m 0644 "$here/mainnet-credential.conf" /etc/systemd/system/lightning-goats-reserve@mainnet.service.d/credential.conf
install -m 0644 "$here/tmpfiles.conf" /etc/tmpfiles.d/lightning-goats-reserve.conf
install -m 0644 "$here/reserve.env.example" /etc/lightning-goats-reserve/mainnet.env
install -m 0644 "$here/mint.env.example" /etc/lightning-goats-reserve/mint-mainnet.env.example
install -m 0755 "$here/add-breez-api-key.sh" "$here/provision-home-credentials.sh" "$base/"
install -m 0644 "$here/wallet/accept.py" "$base/accept.py"
install -m 0644 "$here/solvency.py" "$base/solvency.py"
systemd-tmpfiles --create /etc/tmpfiles.d/lightning-goats-reserve.conf
systemctl daemon-reload
printf 'Installed inactive wallet and mint units from %s\n' "$2"
