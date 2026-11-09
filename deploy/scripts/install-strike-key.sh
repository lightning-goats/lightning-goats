#!/usr/bin/env bash
# Install only the Strike receive-only API key. No webhook setup or API calls.
set +x
set -euo pipefail
umask 077

if [[ ${1:-} == --help ]]; then
    echo 'Usage: sudo bash /home/linuxuser/lightning-goats/install-strike-key.sh [--key-file /root/strike-api-key.txt]'
    echo 'Prompts for one API key or reads a private file; encrypts it and starts no services.'
    exit 0
fi
source_file=
if [[ $# -eq 2 && $1 == --key-file ]]; then
    source_file=$2
elif [[ $# -ne 0 ]]; then
    echo 'Use --key-file PATH or no arguments. Never put the key itself on the command line.' >&2
    exit 2
fi
[[ $EUID -eq 0 ]] || { echo 'Run with sudo bash /home/linuxuser/lightning-goats/install-strike-key.sh' >&2; exit 1; }
command -v systemd-creds >/dev/null

store=/etc/credstore.encrypted
destination="$store/lightning-goats-strike-api-key"
[[ ! -L $store && ! -L $destination ]] || { echo 'Refusing a symlink credential path.' >&2; exit 1; }
install -d -o root -g root -m 0700 "$store"
temporary=$(mktemp -d "$store/.strike-api.XXXXXXXX")
cleanup() {
    unset lg_strike_api_key
    rm -rf -- "$temporary"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

echo 'Use a key with partner.receive-request.create and partner.receive-request.read only.'
if [[ -n $source_file ]]; then
    [[ -f $source_file && ! -L $source_file ]] || { echo 'Key source must be a regular file, not a symlink.' >&2; exit 1; }
    [[ $(stat -c '%u:%a' "$source_file") == 0:600 ]] || { echo 'Key source must be root-owned with mode 0600.' >&2; exit 1; }
    [[ $(stat -c '%s' "$source_file") -le 16384 ]] || { echo 'Key source is unexpectedly large; nothing changed.' >&2; exit 1; }
    lg_strike_api_key=$(cat -- "$source_file")
else
    IFS= read -r -s -p 'Paste the new Strike API key, then press Enter: ' lg_strike_api_key </dev/tty
    printf '\n' >/dev/tty
fi
# Remove accidental surrounding whitespace without printing the key.
lg_strike_api_key="${lg_strike_api_key#"${lg_strike_api_key%%[![:space:]]*}"}"
lg_strike_api_key="${lg_strike_api_key%"${lg_strike_api_key##*[![:space:]]}"}"
[[ -n $lg_strike_api_key ]] || { echo 'Empty key; nothing changed.' >&2; exit 1; }
[[ $lg_strike_api_key != *[$'\r\n\033']* ]] || { echo 'Control characters detected; nothing changed. Enter the key again.' >&2; exit 1; }

printf '%s' "$lg_strike_api_key" |
    systemd-creds encrypt --name=strike-api-key - "$temporary/api-key"
# Compare the decrypted bytes to the imported value without displaying either.
if ! systemd-creds decrypt --name=strike-api-key "$temporary/api-key" - |
    cmp --silent - <(printf '%s' "$lg_strike_api_key"); then
    echo 'Encrypted round-trip comparison failed; nothing changed.' >&2
    exit 1
fi
unset lg_strike_api_key
chmod 0600 "$temporary/api-key"
if [[ -e $destination ]]; then
    [[ -f $destination ]] || { echo 'Existing credential is not a regular file.' >&2; exit 1; }
    cp --preserve=mode,ownership -- "$destination" "$temporary/previous-key"
    backup=$(mktemp "$store/.strike-api-key-backup.XXXXXXXX")
    install -o root -g root -m 0600 "$temporary/previous-key" "$backup"
    echo 'Preserved the previous encrypted API key in a private backup.'
fi
mv -f -- "$temporary/api-key" "$destination"
if command -v restorecon >/dev/null; then
    restorecon "$destination"
fi
echo 'Strike API key installed; encrypted round-trip matches the imported key exactly.'
echo 'No services were started or restarted.'
