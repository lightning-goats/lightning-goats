#!/usr/bin/env bash
set -euo pipefail
umask 077

if (( EUID != 0 )); then
    exec sudo -- "$0" "$@"
fi
if (( $# != 0 )); then
    echo 'Run this script without arguments. It reads the application key from the terminal.' >&2
    exit 2
fi
credential_dir=/etc/credstore.encrypted/lightning-goats-reserve
install -d -o root -g root -m 0700 "$credential_dir"
destination="$credential_dir/breez-api-key.cred"
if [[ -e "$destination" || -L "$destination" ]]; then
    echo 'The Breez application key already has an encrypted credential. Rotate it separately.' >&2
    exit 1
fi
staging=$(mktemp -d "$credential_dir/.breez-key.XXXXXXXX")
trap 'unset breez_application_key; rm -rf -- "$staging"' EXIT
IFS= read -rs -p 'Breez Spark application key: ' breez_application_key </dev/tty
printf '\n' >/dev/tty
if [[ -z "$breez_application_key" || "$breez_application_key" =~ [[:space:]] ]]; then
    echo 'Enter a nonempty key without whitespace.' >&2
    exit 2
fi
printf '%s' "$breez_application_key" | systemd-creds encrypt --with-key=host --name=breez-api-key - "$staging/key.cred"
unset breez_application_key
chmod 0600 "$staging/key.cred"
ln -- "$staging/key.cred" "$destination"
printf 'Encrypted application credential installed at %s\n' "$destination"
