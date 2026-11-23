#!/usr/bin/env bash
# Run on the VPS: sudo bash deploy/scripts/provision-strike-credentials.sh
# Secret values are read from the terminal, never arguments or plaintext files.
set +x
set -euo pipefail
umask 077

if [[ ${1:-} == --help ]]; then
    echo 'Usage: sudo bash provision-strike-credentials.sh [--api-key-only]'
    echo 'Encrypt missing Strike credentials for lightning-goatsd; never overwrite or start services.'
    exit 0
fi
if [[ $# -gt 1 || ( $# -eq 1 && $1 != --api-key-only ) ]]; then
    echo 'Unknown argument. Use --help.' >&2
    exit 2
fi
if [[ $EUID -ne 0 ]]; then
    echo "Run this script with sudo bash; root must own the encrypted credentials." >&2
    exit 1
fi
command -v systemd-creds >/dev/null
[[ -r /dev/tty && -w /dev/tty ]] || { echo 'An interactive terminal is required.' >&2; exit 1; }

store=/etc/credstore.encrypted
[[ ! -L $store ]] || { echo 'Refusing a symlink credential directory.' >&2; exit 1; }
install -d -o root -g root -m 0700 "$store"
names=(strike-api-key)
[[ ${1:-} == --api-key-only ]] || names+=(strike-webhook-secret)
missing=()
for name in "${names[@]}"; do
    destination="$store/lightning-goats-$name"
    if [[ -e $destination || -L $destination ]]; then
        [[ -f $destination && ! -L $destination && $(stat -c '%u:%a' "$destination") == 0:600 ]] || {
            echo "Existing credential has unsafe ownership, permissions or type: $destination" >&2
            exit 1
        }
        echo "Preserving existing credential: $destination"
    else
        missing+=("$name")
    fi
done
if [[ ${#missing[@]} -eq 0 ]]; then
    echo 'Credentials already exist; nothing changed.'
    exit 0
fi

temporary=$(mktemp -d "$store/.lg-strike.XXXXXXXX")
cleanup() {
    unset lg_strike_secret
    rm -rf -- "$temporary"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

echo 'Use a Strike key with partner.receive-request.create and partner.receive-request.read only.'
echo 'Do not use a spending/withdrawal key. The webhook signing secret is a separate value.'
for name in "${missing[@]}"; do
    case $name in
        strike-api-key) prompt='Strike receive-only API key: ' ;;
        strike-webhook-secret) prompt='Strike webhook signing secret: ' ;;
    esac
    IFS= read -r -s -p "$prompt" lg_strike_secret </dev/tty
    printf '\n' >/dev/tty
    [[ -n $lg_strike_secret ]] || { echo 'Empty credential; nothing installed.' >&2; exit 1; }
    printf '%s' "$lg_strike_secret" |
        systemd-creds encrypt --name="$name" - "$temporary/$name"
    unset lg_strike_secret
    chmod 0600 "$temporary/$name"
done
for name in "${missing[@]}"; do
    destination="$store/lightning-goats-$name"
    # An exclusive hard link prevents replacing a credential created meanwhile.
    ln -- "$temporary/$name" "$destination"
    echo "Installed encrypted credential: $destination"
done
command -v restorecon >/dev/null && restorecon "$store" "$store"/lightning-goats-strike-* || true
echo 'Done. No service was started or restarted.'
