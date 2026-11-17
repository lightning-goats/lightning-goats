# Dedicated Cashu seed preparation

The operator has backed up a new, dedicated 12-word Cashu seed. Never enter the
phrase in chat, command arguments, environment variables or repository files.

Use the standalone [offline script](../../deploy/scripts/add-cashu-seed.py) on a
trusted operator machine with Python 3 and a Unix terminal, as a non-root user:

```sh
python3 add-cashu-seed.py
```

It prompts twice with terminal echo disabled, validates the English BIP39 words
and checksum, and derives the standard 64-byte BIP39 seed. The compressed English
wordlist comes from the checksum-verified locked `bip39` 2.2.2 archive; its decoded
SHA-256 is checked before use. No installation or network download is needed.

The default output is
`~/.local/share/lightning-goats-cashu/seed-staging/wallet-seed.bin`.
New directories are private, and the file is mode `0600`, owned by the caller.
Symlink paths, unsafe writable ancestors and nonprivate output directories
refuse. Publication is atomic and never replaces an existing file or link.
Core dumps and insecure terminal-input fallback are disabled. Python does not
provide a guarantee of erasing every in-memory copy of the input.

Use `--output /private/path/wallet-seed.bin` for another private destination.
Use `--passphrase` only if the mnemonic was created with an additional BIP39
passphrase; a wallet PIN is a different setting. Without that option, the BIP39
passphrase is empty. The original offline word backup remains necessary.

This prepares seed material; it does not initialize a wallet, open journals,
change configuration, start services, announce receiving, or contact a provider.
Provisioning into the isolated runtime remains part of reviewed deployment:
first narrow temporary deployment privileges and qualify the runtime ownership
boundary, then install these exact 64 bytes as `wallet-seed.bin` in the configured
private state directory. Never put the final production seed under a development
identity as a substitute for that boundary. The default staging path is for an
operator machine, not the production daemon's state directory.

The separate receiving/P2PK key, historical keys, pending operation material,
NIP-46 client credentials and coordinated core/wallet journals need their own
backups. This file is not a full-wallet recovery archive; see the
[recovery inventory](cashu-recovery-inventory.md).

Validation uses the public [BIP39 reference vectors](https://github.com/trezor/python-mnemonic/blob/master/vectors.json)
and [BIP39 specification](https://github.com/bitcoin/bips/blob/master/bip-0039.mediawiki).
Eight tests pass, including an actual pseudo-terminal check that neither phrase
entry is echoed, private-file permissions, overwrite/symlink refusal, interrupted
write cleanup and redacted accidental command arguments. Tests contain only
public/synthetic material; the operator's phrase was never supplied to Codex.
