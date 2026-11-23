#!/usr/bin/env bash
set -euo pipefail
umask 077

[[ $EUID -eq 0 && $# -eq 1 ]] || { echo 'Usage: sudo provision-home-credentials.sh /absolute/new/recovery.tar.gpg' >&2; exit 2; }
backup=$1
[[ "$backup" = /* && ! -e "$backup" && ! -L "$backup" ]]
credential_dir=/etc/credstore.encrypted/lightning-goats-reserve
bundle=/opt/lightning-goats-reserve/current
install -d -m 0700 "$credential_dir"
for name in mainnet-seed mainnet-mint-seed mainnet-project-quote-token; do
    [[ ! -e "$credential_dir/$name.cred" && ! -L "$credential_dir/$name.cred" ]]
done
staging=$(mktemp -d /run/lg-reserve-recovery.XXXXXXXX)
trap 'unset passphrase confirm; rm -rf -- "$staging"' EXIT
IFS= read -rs -p 'Offline recovery archive passphrase: ' passphrase </dev/tty
printf '\n' >/dev/tty
IFS= read -rs -p 'Confirm recovery archive passphrase: ' confirm </dev/tty
printf '\n' >/dev/tty
[[ ${#passphrase} -ge 16 && "$confirm" == "$passphrase" ]] || { echo 'Passphrases must match and contain at least 16 characters.' >&2; exit 2; }
printf '%s\n' "$passphrase" > "$staging/passphrase"
unset passphrase confirm
PYTHONPATH="$bundle/site-packages" "$bundle/python/bin/python3.11" - "$staging" <<'PY'
import secrets
import sys
from pathlib import Path
from mnemonic import Mnemonic

directory = Path(sys.argv[1])
english = Mnemonic("english")
(directory / "reserve-seed").write_text(english.generate(strength=256))
(directory / "mint-seed").write_text(english.generate(strength=256))
(directory / "project-quote-token").write_text(secrets.token_urlsafe(48))
PY
install -d -m 0700 "$staging/gpg"
tar -C "$staging" -cf "$staging/recovery.tar" reserve-seed mint-seed project-quote-token
gpg --homedir "$staging/gpg" --batch --pinentry-mode loopback --passphrase-file "$staging/passphrase" \
    --cipher-algo AES256 --symmetric --output "$staging/recovery.tar.gpg" "$staging/recovery.tar"
gpg --homedir "$staging/gpg" --batch --pinentry-mode loopback --passphrase-file "$staging/passphrase" \
    --decrypt "$staging/recovery.tar.gpg" | cmp - "$staging/recovery.tar"
(set -o noclobber; cat "$staging/recovery.tar.gpg" > "$backup")
for entry in 'reserve-seed mainnet-seed' 'mint-seed mainnet-mint-seed' 'project-quote-token mainnet-project-quote-token'; do
    read -r credential destination <<< "$entry"
    systemd-creds encrypt --with-key=host --name="$credential" "$staging/$credential" "$staging/$destination.cred"
    chmod 0600 "$staging/$destination.cred"
    install -m 0600 "$staging/$destination.cred" "$credential_dir/.$destination.new"
    ln -- "$credential_dir/.$destination.new" "$credential_dir/$destination.cred"
    rm -- "$credential_dir/.$destination.new"
done
printf 'Independent credentials installed. Encrypted offline recovery archive written to %s\n' "$backup"
