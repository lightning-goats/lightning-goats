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

The sections below describe the implemented balance, transport and durable
state APIs. Remaining implementation includes runtime assembly, dedicated credential
configuration and installation/restore procedures. The NIP-17
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
observations between concurrent processes. Until runtime assembly and operational acceptance are complete, the
alert-delivery gate remains open.

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
has stopped. These transport tests do not establish durable alert episodes; the separate
state regressions below cover those. Runtime wiring and operational provisioning
remain required.

## Durable episode and outbox API

`private_alert::AlertStore` uses a separate WAL/FULL SQLite file. Explicit
`initialize` is preparation-only and refuses existing state; runtime `connect`
refuses absent tables/singleton, unknown schema versions, unrelated tables or a
changed policy binding. Missing state never silently rearms alerting. The
binding covers the protected threshold, recipient, inbox relay list and reviewed
account/config generation label. That label is not independent proof of the
credential's effective provider account: provisioning must establish that.

`poll` acquires BEGIN IMMEDIATE before invoking the authoritative Strike read.
A high observation with an armed episode encrypts first, then atomically inserts
the exact completed wrapper and disarms the episode. Repeated high observations
produce no new wrapper. Only a successful later below-threshold read rearms it.
Read, encryption and database failures leave the previous state intact. No
balance, threshold or plaintext message is stored in an event row; the policy
binding and ciphertext metadata remain private operational data. At most 128
pending wrappers are admitted; capacity failure occurs before encryption.

`deliver_next` serializes with observation and other publishers, verifies the
saved event identity/signature/recipient and publishes the same saved bytes.
Failure retains the row and increments a bounded attempt counter. Success deletes
only that row; it does not rearm the threshold. A lost response or DB failure
after relay acceptance causes replay of the same event ID, never re-wrapping.
No public event/outbox table is created or used. Network operations under this
separate DB lock use the existing bounded provider/nak deadlines; another
process can time out acquiring SQLite and must retry later, without an early
provider read. This lock never covers the financial or gateway ledger.

Unit regressions use independent file-backed connections, mocked balances and
explicit non-cryptographic nak fixtures. They cover episode/rearm, database
reopen, exact retry bytes, overlapping reads, read/encryption/insert failures,
publication followed by DB failure, bounded backlog, changed policy, lost state
and rejection of the financial database. The real transport proof remains the
separate pinned-bunker test. These tests do not prove cross-process crash or
full-host anti-rollback; operational restore must reconcile private alert state
with its protected policy and keep the worker stopped until reviewed.

## Worker and protected policy API

`run_private_alert_worker` is an explicitly invoked library worker. It attempts
existing ciphertext delivery before an authoritative observation, so provider
failure does not prevent pending delivery. Separate schedules normally poll every
30 seconds and deliver one pending row every 15 seconds. Failures double each
schedule's delay independently up to 300 seconds; successful operations reset it.
Deadlines start at operation completion, avoiding catch-up bursts. Operations
remain sequential under the private store's lock; provider/nak timeouts can delay
both schedules. This is an alert, not an automatic balance cap or sweep service.

A stop signal or closed shutdown channel prevents subsequent operations. An
in-flight transaction/subprocess is allowed to complete using its existing
bounded deadline, rather than cancelling midway through cleanup. Logs contain
only generic observation/delivery availability, without errors, balance, policy,
recipient or subprocess output. Tests cover retry after provider failure,
completion during shutdown, no I/O when already stopped, capped/reset delays and
scheduling from completion.

`AlertPolicy::from_systemd_credential` reads only `private-alert-policy` from the
absolute systemd credential directory. The file must be a non-symlink regular
0400 file, at most 16 KiB, read within three seconds. Metadata is compared before
and after opening, reads are bounded, raw bytes are zeroized and parsing errors
are replaced with generic messages. JSON fields are exactly `threshold_sats`,
`recipient` (lowercase hexadecimal), `inbox_relays`, and `account_binding`;
unknown/duplicate/missing fields fail. No values default to public announcement
configuration. Permission, symlink, size and parser-redaction regressions use
synthetic local files. This does not establish the trusted source or effective
owner of systemd's credential directory: the reviewed unit and installation
acceptance must establish that boundary.

## CLI and inactive unit candidate

`lightning-goatsctl private-alert initialize|check|run` now assembles the runtime
without loading the financial application's config or opening its ledger.
Initialize/check require only protected policy/runtime credentials and perform
no provider/signer calls. Run requires separate balance-read and NIP-46 client
credentials, opens existing state, and handles SIGTERM/SIGINT through the worker's
bounded cleanup. The state binding additionally covers provider URL and signer
identity/relay assignment; changes require reconciliation. CLI errors are generic.

`tests/private_alert_cli.rs` executes the real CLI against temporary protected
files. It covers offline initialization/check without keys or financial config,
missing state/key rejection, provider/signer reassignment and redacted runtime
validation. A loopback HTTP provider plus non-cryptographic nak process fixture
checks actual balance polling, exactly one wrap/publication and high-episode
persistence after process restart. This does not replace the real-bunker proof.

The archive now requires the inactive `lightning-goats-private-alert.service`
candidate. Syntax verification is not actual service isolation acceptance.
Dedicated identity creation, encrypted-credential provisioning, actual unit
sandbox checks, synthetic service rehearsal, operational account/scopes and
inbox delivery acceptance remain open. See
[private alert operations](../deployment/private-alert-operations.md) for the
preparation, activation and restore boundaries. No unit is installed or activated
by these repository changes.
