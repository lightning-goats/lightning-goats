# Household nak with a dedicated Cashu client

This candidate builds from the pinned household nak v0.20.7 source and its
existing dependency locks. It adds one optional public key flag to the bunker.
The project signing key stays in the household's existing credential boundary.

The bunker accepts the dedicated public key through `--cashu-client`. It
checks the authenticated request before it reaches the signing backend. The
scope permits connecting to the configured owner, querying its public key,
ping, signing kinds 10019 and 7376, and NIP-44 encryption or decryption with
the owner as the peer. It returns an encrypted NIP-46 refusal for other
requests. A key in the general authorization list cannot also use this flag.
Ordinary clients continue to use nak's existing authorization.
[NIP-46](https://github.com/nostr-protocol/nips/blob/master/46.md)

```sh
python3 tools/cashu-nak-bunker/build.py --build-directory /absolute/private/build-directory
```

The build verifies the upstream archives and patch, compares Go vet findings
against the pinned unpatched source, and tests the authorization boundary and
its signed responses. It produces `nak-cashu-bunker` and `build-result.json`.
The wire test exercises the candidate with synthetic keys over an isolated
loopback relay, including an ordinary authorized client and an unknown client.

A reviewed household configuration passes `--cashu-client PUBLIC_KEY` alongside
the existing relays and general client flags. The household activation procedure
requires explicit approval for the exact binary and configuration. The repository
production HOLD still applies. Preserve the current household signer and its
existing client settings for rollback.
