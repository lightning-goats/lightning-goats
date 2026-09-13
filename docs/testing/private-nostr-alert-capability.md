# Private alert signer capability

Preparation evidence for issue #16. No alert worker or production DM is enabled.
The public kind-1 signing/outbox path is unchanged.

The pinned nak v0.20.6 binary supports `gift wrap` using its configured NIP-46
keyer for NIP-44 rumor encryption and kind-13 seal signing, then a fresh local
key for the kind-1059 wrapper. Its default decoupled-key resolution performs
relay discovery. An identity-bound alert must pass both
`--use-our-identity-key` and `--use-their-identity-key`, using an explicitly
approved recipient. Never substitute a discovered profile/key during an alert.
Source: https://github.com/fiatjaf/nak/blob/v0.20.6/gift.go
Protocol: https://github.com/nostr-protocol/nips/blob/master/17.md

`deploy/tests/test_private_nostr_capability.py` verifies the exact installed
binary SHA before execution and refuses any namespace with links other than
loopback. It starts only a local relay and the shipped bunker wrapper using
public test scalars 01/02/03. Child environments are cleared. It supplies the
kind-14 plaintext on stdin, verifies the outer signature, explicitly decrypts
both layers with the synthetic recipient, verifies the seal signature/sender,
and checks the rumor hash, sender, recipient, content, timestamp and unsigned
status. Explicit `decrypt` avoids `gift unwrap`'s separate discovery behavior.
A query confirms no message kinds 1/13/14/1059 were published; the relay carries
only signer-protocol traffic. All children are killed/reaped in cleanup.

The dedicated Real NIP-46 acceptance workflow runs this test with the same
SHA-pinned release as the existing public outbox test and preserves its log.
Ordinary deployment discovery skips it unless LG_TEST_NAK is explicitly set.
For an approved isolated local run, use a fresh `unshare --net` namespace, bring
only loopback up, drop back to the development user and invoke:

```sh
LG_TEST_NAK=/path/to/pinned/nak python3 -B -m unittest discover \
  -s deploy/tests -p test_private_nostr_capability.py -v
```

VPS local result: PASS. An initial test harness passed ciphertext on stdin to
`nak decrypt`; that command requires a positional ciphertext argument. The
failed run is retained privately; the corrected run verified both layers.
This does not establish a production signer identity, effective credential
permissions, recipient client acceptance or approved DM relay reachability.

Remaining implementation: distinct ciphertext-only durable outbox, authoritative
read-only Strike balance observation, durable threshold episodes/rearm, bounded
subprocess handling using the application boundary, failed-encryption rejection,
exact-byte retry after restart and protected recipient/relay policy. The NIP-17
relay policy must be reconciled with the operator's approved recipient inbox
relays before delivery; announcement relays are not implicitly DM relays.
Missing encryption/signing must fail before any message publication. Public
payment/feed tests must remain unchanged. No real message, payment or feeding
is authorized by this capability proof. Production HOLD.

## Authoritative balance input

`StrikeClient::btc_balance` adds only `GET /v1/balances` using the existing
bounded, redirect-disabled provider client. A project balance credential must
include the read-only `partner.balances.read` scope; this change neither loads
new credentials nor broadens effective scopes. The API method is not yet wired
to a runtime worker.

The typed result exposes BTC `current` converted exactly to whole satoshis.
Strike defines this as including pending amounts; `available` may be lower.
Deprecated `total`/`outgoing` are never used, and fiat balances are not converted
or represented as a BTC-equivalent exposure. Deployment must explicitly confirm
that the BTC current-balance policy matches the dedicated account configuration.
An absent/duplicate BTC row, missing current field, non-string/negative/exponent/
whitespace/overflow/fractional-satoshi value, auth failure, redirect or oversized
response fails without a balance. Zero is accepted only as an explicit valid
BTC current amount. Errors omit provider values, and the result intentionally
implements neither Debug nor Serialize.

Sources inspected 2026-09-13:
- https://docs.strike.me/api/get-account-balance-details/
- https://docs.strike.me/api/ (2024-07-19 balance-field deprecation)

`tests/strike_balance.rs` exercises the real HTTP client against harmless mock
responses, asserting one authenticated GET and no alternate-path/redirect request.
No store or public event/outbox operation is part of this provider read. The
alert worker must acquire its durable observation/episode serialization before
calling it; accepting externally prefetched balances could reorder high/low
observations between concurrent processes. Until that worker, encrypted outbox
and runtime acceptance are implemented, the alert-delivery gate remains open.

## Application private transport

`NakClient::wrap_private_message` now constructs kind 14 through the existing
bounded subprocess runner, forces both identity-key options, and returns a
separate `PrivateGiftWrap`. It validates exact outer fields, recipient tag,
kind 1059, signature and NIP-44 v2 payload framing. Encryption remains in the
pinned nak/bunker implementation; framing validation alone is not a MAC check.
The application cannot decrypt a recipient's message without that private key.

`restore_private_wrap` verifies saved ciphertext and preserves its exact bytes.
`publish_private_wrap` takes an explicit inbox relay list, validates it before
subprocess execution, verifies the saved event and checks the publication echo.
Only wrapping receives NIP-46 credentials. Verification/retry use no signer;
there is no fallback to the configured public announcement relays. The public
kind-1 validators are unchanged and reject private wrappers. Unknown outer
fields are rejected rather than accidentally persisting a plaintext side field.
Private payload/error values have no automatic Debug/Serialize representation.

The already locked base64 0.22.1 crate is now a direct dependency for bounded
NIP-44 framing checks; no dependency version was changed. Protocol reference:
https://github.com/nostr-protocol/nips/blob/master/44.md

`tests/private_nostr_transport.rs` uses explicit non-cryptographic subprocess
fixtures for encryption failure, unexpected fields, wrong kind/recipient,
plaintext/invalid framing, invalid signature, publication failure/alteration,
credential separation, explicit relays and exact retry input. The real-nak Rust
acceptance additionally exercises the actual application wrapper, decrypts its
layers using synthetic keys and retries the same wrapper after the real bunker
has stopped. These tests do not establish durable alert episodes; the separate
SQLite outbox/worker and operational provisioning are still required.
