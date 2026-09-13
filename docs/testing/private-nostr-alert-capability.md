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
