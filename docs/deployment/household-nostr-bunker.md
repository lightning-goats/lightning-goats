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
ArtX. The browser's Damus connection failed, and its HTTPS metadata returned 403.
A subsequent Go-style TLS/WebSocket handshake as daemon UID 985 returned 101
with a verified WebSocket accept value, distinguishing that browser limitation
from actual service reachability. Chaima's NIP-11 metadata was readable
as the daemon user and advertised neither payment nor restricted-write requirements.
None of these read-only checks proves public note acceptance/readback.

The candidate also removes paused-status prefixes from overlay scrolling text.
The prominent paused/queued banner remains. Information/weather retain no Nostr
audience, including while paused; eligible payment and confirmed-feed Nostr text
keeps its frozen queue context. CyberHerd events remain future implementation.

The fresh audit identified an existing SQLite dependency advisory; see
[its scoped assessment](../security/sqlite-wal-advisory-20261109.md). That finding
remains open and unignored. The existing risk-tiered policy permits this scoped
relay/presentation update under the verified private-file conditions; it does
not fix SQLite or establish a clean audit. Initial Primal/client staging above
used the previously installed binary; final installation is recorded below.

## Public activation boundary

Preparation retained `canary` until the operator's explicit public activation
recorded below. Before switching to `active`, preserve the paid ledger, verify
no unresolved feed, and check the message cursor is caught up so earlier test
activity is not published.
Keep the existing confirmed-feeding behavior, 10-sat announcement threshold,
overlay-only informational/weather audiences and exact signed-event retries.
No Lightning payment, added credit or physical feeding is needed for the
signing-only connection check.

## Installed scoped update — 2026-11-09

Native source `333040010c8f4c632174c73c6ee3b9f5ee303929` was packaged with archive
SHA256 `6aa1e6c460b970ac6b0860aa3f460c4e98fadca829784cc824b091bd345bc902`.
Formatting and strict Clippy passed. The regular suite passed 260 tests with the
dedicated real-nak case ignored there; that case was then executed separately in
a fresh loopback-only namespace and passed, including exact durable retry without
the signer. Archive completeness, checksums, source identity and all binary help
smokes passed. The audit remains flagged as assessed above; it is not a clean gate.

Native backup: `/var/backups/lightning-goats/receive-upgrade-20261109T140606Z`.
Final four-relay/client configuration backup:
`/var/backups/lightning-goats/household-nostr-20261109T140621Z`.
Installed daemon SHA256 is
`629df18a46fc3a0ef1052f802174d10e217e3687fff0eeef770a84525e5b7e8d`.
Readback verified all four publication destinations, independent Primal bunker
transport, a loaded client-only credential, active canary service and zero
restarts. Credit remained 1142 sats; feed attempts/debits and Nostr outbox rows
remained zero. Message cursor and latest durable event both read 651.

Actual public Chromium verification replayed nine existing messages in a private
observer (eight interface-info and one weather); every scrolling message omitted
the paused prefix. The visible paused banner still showed one queued feeding,
and there were no JavaScript errors. No credit, Lightning payment, physical
feeding, public payment/feed post, home switch, DNS or firewall change was used.
The static overlay assets were unchanged. Public note delivery/readback on the
four relays remains for the separately selected public activation step.

## Public publishing enabled — 2026-11-09 14:17:09 UTC

The operator explicitly requested "Enable Nostr publishing." The reviewed
`deploy/scripts/enable-nostr.py` helper changed only `service.mode` from canary
to active, after quiescing the daemon and verifying cursor/latest-event equality
and an empty unpublished outbox. No cursor, financial row or historical event
was edited. The cutoff is event **658**; earlier activity remains unpublished.
The focused configuration check preserved unrelated modes/amounts and rejected
non-canary transformation. Python syntax and whitespace checks passed; no Rust
source, binary, dependency, credential, relay or audience changed in this step.

Consistent private backup:
`/var/backups/lightning-goats/nostr-enable-20261109T141708Z`.
Root-owned `/etc/lightning-goats/NOSTR-ACTIVATION.json` records the cutoff, time,
relay selections and backup. On activation failure the helper restores only its
configuration change and restarts canary; paid/event/outbox state is retained.
Repeating the helper while already active makes no changes.

Live readback: active service, runtime user `lightning-goats`, zero restarts,
1142 sats, one funded feeding plus 142 remainder, no unresolved/feed attempts,
and unchanged local override ON / remote enable ON. Public posting is enabled
for new eligible payment/operator-credit events (10-sat minimum) and confirmed
feedings, using the household signer and all four configured announcement relays.
Information/weather remain WebSocket-only; CyberHerd producers are future work.
No test credit, payment, physical feeding or public test note was created to
activate it. Actual public note acceptance/readback awaits the next qualifying
payment/credit or confirmed feeding; service activation alone is not that evidence.
