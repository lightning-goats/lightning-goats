#!/usr/bin/env bash
# Only installs NEW dedicated paths. It does not upgrade, change networking,
# create credentials/accounts, load/start/enable units, or operate MoneroPay.
set -euo pipefail
[[ ${1:-} == --install-new-inactive && $# == 1 ]] || { echo 'Usage: sudo bash install-inactive.sh --install-new-inactive' >&2; exit 2; }
[[ $EUID == 0 ]] || exit 2
cd -- "$(dirname -- "$(readlink -f -- "$0")")"
# Require a root-owned immutable-to-the-service approved bundle, including ancestors.
directory=$PWD
while :; do
  [[ $(stat -c %u -- "$directory") == 0 ]] || exit 1
  mode=$(stat -c %a -- "$directory")
  (( (8#$mode & 0022) == 0 )) || exit 1
  [[ $directory == / ]] && break
  directory=$(dirname -- "$directory")
done
[[ -z $(find . -xdev \( -type l -o ! -uid 0 -o -perm /022 \) -print -quit) ]] || exit 1
sha256sum --check --strict SHA256SUMS
getent passwd lightning_goats_monero >/dev/null
getent group lightning_goats_monero >/dev/null
[[ $(id -u lightning_goats_monero) != 0 && $(getent group lightning_goats_monero | cut -d: -f3) != 0 ]] || exit 1
for target in /opt/lightning-goats-monero /etc/lightning-goats-monero /var/lib/lightning-goats-monero; do
  [[ ! -e "$target" && ! -L "$target" ]] || { echo 'Refusing existing installation/state; use reviewed upgrade plan.' >&2; exit 1; }
done
for unit in lightning-goats-monero-bridge.service lightning-goats-monero-sync.service lightning-goats-monero-sync.timer; do
  [[ ! -e /etc/systemd/system/$unit && ! -L /etc/systemd/system/$unit ]] || exit 1
  if systemctl is-active --quiet "$unit"; then echo 'Refusing active unit' >&2; exit 1; fi
done
# No secret-bearing configuration is accepted from an untrusted source directory.
# Operator reviews this staging bundle and source checksum before running as root.
install -d -o root -g root -m 0755 /opt/lightning-goats-monero
install -o root -g root -m 0755 lightning-goats-monero-bridge sync-probe.py /opt/lightning-goats-monero/
install -d -o root -g lightning_goats_monero -m 0750 /etc/lightning-goats-monero
install -d -o root -g root -m 0700 /etc/lightning-goats-monero/credentials
install -o root -g root -m 0644 config.toml.example sync.json.example /etc/lightning-goats-monero/
install -d -o lightning_goats_monero -g lightning_goats_monero -m 0700 /var/lib/lightning-goats-monero
install -o root -g root -m 0644 lightning-goats-monero-bridge.service lightning-goats-monero-sync.service lightning-goats-monero-sync.timer /etc/systemd/system/
echo 'Installed INACTIVE examples. No config/token generated; no daemon-reload/start/enable/network command executed.'
