# Household NIP-46 bunker — 2026-11-09

The operator selected the existing household bunker on `10.8.0.6`, documented in
[Earthship's deployment](https://github.com/santyr/earthship-ui/tree/main/deploy/nostr-bunker).
NIP-46 traffic uses public relays; no new private-network port, SSH authorization,
VPS bunker, signer credential import or household network change is needed.

The existing `nostr-bunker@earthship-operator.service` signs as
`669ebbcccf409ee0467a33660ae88fd17e5379e646e41d7c236ff4963f3c36b6`, matching
the current Lightning Goats announcement config and herd NIP-05 identity.
Do not substitute the household Hex sending identity or reuse Sat's client key.

## Prepared VPS credential

`deploy/scripts/prepare-household-nostr-client.py` generated and verified a
host-encrypted, root-owned mode-0600 `nostr-client-key` credential at
`/etc/credstore.encrypted/lightning-goats-nostr-client`. Its public identity is:

```text
d7a195216dec31cbdd3a98ab5d99b2865a7e74c7ad6d83144570f98b681c8256
```

A second actual run reused the same credential/public key. It does not alter
the daemon, sign a note, publish anything or import the project signing key.
The installed nak remains the checksum-qualified v0.20.6 binary. The old
`install-nostr-key.sh` local-signer draft is not used for this household setup.

## Household action

The operator requested a command rather than new VPS administrative access.
The self-contained public script is saved on the VPS at
`/home/linuxuser/lightning-goats/authorize-household-nostr-client.sh`. Copy it
to the household host and run there:

```sh
bash authorize-household-nostr-client.sh
```

Its Python body is `deploy/scripts/authorize-household-nostr-client.py`, with
the public key above supplied as its only argument. It requires the expected
root-owned environment file and an active existing Earthship bunker, backs up
the public environment, preserves every existing allowlist client and all
relay/digest settings, adds the dedicated client and restarts only that bunker
instance. A repeat is a no-op. It changes neither a signing credential nor boot
enablement and publishes nothing. It prints only public settings/service state.
The brief signer restart may interrupt existing requests; their clients can retry.

Focused verification established exact retention of the existing client, relay
and binary-digest settings; idempotent repeat; rejection of malformed/injected
client keys and duplicate allowlist definitions. Python parsing, shell syntax
and whitespace checks passed. These are not live household acceptance.

## Connection verification and initial staging

The operator ran the household script, retaining backup
`/etc/nostr-bunker/earthship-operator.env.lg-20261109T131749Z`. Readback confirmed
both client public keys in the running process arguments. The selected instance
was active/running with zero restarts; the two older named Lightning Goats
instances were inactive. The household's existing offline verifier matched the
signing credential to the operator's existing DM recipient.

Two actual signing-only challenges through `wss://relay.primal.net` succeeded as
daemon UID 985 using its encrypted client credential. Expected author, exact
content/tags, canonical hash and signature verified; no public kind-1 note was
sent. Live response observation verified one acknowledgement and one successful
result. The initial local precheck passed empty stdin to `nak key public`; that
helper error is corrected to explicit key stdin. The preparation helper also
re-derived the same existing client identity without replacing its credential.

The combined signing relay check returned an authorization failure, while
individual nos.lol and Damus signing attempts timed out. Cause is unproven;
these are not evidence that the verified household key/allowlist is incorrect,
nor proof that kind-1 publication is refused. Primal is the qualified NIP-46
transport. No household relay or credential changes were made during diagnostics.

Initial VPS staging retained backup
`/var/backups/lightning-goats/household-nostr-20261109T134003Z`, configured Primal,
and loaded the client-only systemd drop-in. Readback: active daemon, zero restarts,
canary mode, 1142 sats (one funded feeding, 142 remainder), no feed attempts/debits,
and empty Nostr outbox. No local signing credential exists. Assembled systemd
validation passed. Do not print bunker output, pairing URLs or process environments.

## Requested announcement destinations

The operator selected these alongside Primal:

- `wss://nostr.chaima.info/`
- `wss://relay.damus.io/`
- `wss://relay.artx.market/`

The candidate sets `nostr.bunker_relays = ["wss://relay.primal.net"]` independently
from `nostr.relays`, which contains all four public destinations. An omitted
`bunker_relays` preserves legacy shared-list behavior; explicit empty/insecure
transport lists are rejected. Signing receives the client credential; publication
sends the exact persisted event to all configured destinations without contacting
the bunker. Subprocess regressions cover that separation and configuration tests
cover legacy parsing and invalid lists. The installer permits only these two
relay-field changes and the credential drop-in while preserving canary mode.

Read-only WebSocket probes connected and received EOSE from Primal, Chaima and
ArtX. The browser's Damus connection failed, and its HTTPS metadata returned 403;
this remains a qualification limitation. Chaima's NIP-11 metadata was readable
as the daemon user and advertised neither payment nor restricted-write requirements.
None of these read-only checks proves public note acceptance/readback.

The candidate also removes paused-status prefixes from overlay scrolling text.
The prominent paused/queued banner remains. Information/weather retain no Nostr
audience, including while paused; eligible payment and confirmed-feed Nostr text
keeps its frozen queue context. CyberHerd events remain future implementation.

The fresh audit identified an existing SQLite dependency advisory; see
[its scoped assessment](../security/sqlite-wal-advisory-20261109.md). The new
native candidate is not installed pending that gate's disposition. Initial
Primal/client staging above uses the previously installed binary.

## Public activation remains separate

The daemon is still `canary`; public Nostr remains disabled and its outbox is
empty. Public messaging activation remains a separate operator step. Before
switching to `active`, preserve the paid ledger, verify no unresolved feed, and
check the message cursor is caught up so earlier test activity is not published.
Keep the existing confirmed-feeding behavior, 10-sat announcement threshold,
overlay-only informational/weather audiences and exact signed-event retries.
No Lightning payment, added credit or physical feeding is needed for the
signing-only connection check.
