# CyberHerd household signing checks — 2026-11-10

Checker source `20b9f60f46dc390c43b07531e5f8947577b401fc` adds a bounded
[non-admin signing-only helper](../deployment/cyberherd-nostr-clients.md#signing-only-qualification)
and two isolated one-shot unit templates. It supports only membership kinds
1/1311 and payout-signing kind 9734. It publishes no project event, requests no
invoice, loads no financial credential and cannot enable an application worker.

Six focused local tests passed. In
[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42856844331),
job `129972088289`, all six passed again; discovery ran 194 tests with 36 environment
skips, plus 31 separate successful root tests. All five deployment jobs passed,
including the existing native worker fixture. The
[CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42856843983),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42856843974)
and [synthetic real-NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42856843971)
workflows also passed. Rust runtime source is unchanged by this checker.

The operator reports that the household environment contains both prepared
CyberHerd public clients alongside existing clients, with the selected bunker
active/running and zero restarts. The authorization helper reported that the
clients were already present and did not restart it. That establishes file
configuration and reported service health, not the running process's loaded
allowlist or successful client authorization.

On the VPS, the committed helper and public-client substitutions were assembled
as data, independently hashed and installed into a new protected prefix.
Assembled unit verification first encountered restricted-sandbox socket errors;
the same read-only validation outside that restriction passed. No unit was
started until validation and installed-byte hashes matched.

Each actual non-admin signing-only unit loaded just its own encrypted client
credential. Local pinned-nak public derivation matched the expected membership
or payout-signing identity, so neither attempt failed on parsing or mismatched
stored credential bytes. Both NIP-46 requests through the previously qualified
Primal transport then returned **unauthorized**. Membership kinds 1/1311 and payout
kind 9734 therefore remain unqualified. No signature, public project post,
invoice or payment success is claimed. Raw helper output, keys and signed bytes
were withheld; only sanitized failures were retained privately.

Original role journal hashes and all original service states matched before and
after both attempts. The existing receive process remained unchanged, and the
three production CyberHerd units remained masked/inactive. Temporary check units
are terminal failed, with no automatic restart. Their reviewed files and private
evidence are retained pending the operator's household reload; no repeated
attempt has been made without a state change.

The operator subsequently confirmed restarting only the existing household
bunker. One fresh Primal retry per client produced a membership timeout and a
payout-client authorization refusal. Membership-only checks on the other two
approved household transports, nos.lol and Damus, timed out. No payout check on
those alternate transports was executed. The full expected kinds remain
unqualified; no partial signature or successful publication is inferred from a
timeout. A stale allowlist remains an unproven hypothesis, not the established
cause. No key was regenerated or household setting changed by these checks.

All check processes were terminal, with no automatic retry. Original role-journal
hashes and all original service states still matched after these attempts. The
five exact temporary unit copies were removed, including one alternate payout
unit that was staged but never run. Private reports retain the failures and
`household_qualified=false`; the pinned non-admin helper remains available.

Next evidence is the household's actual loaded public clients. A separate
read-only script is prepared at
`/home/linuxuser/lightning-goats/check-household-cyberherd-clients.sh` on the VPS,
from `deploy/scripts/check-household-cyberherd-clients.py`. Its three disposable
tests passed locally: only explicit authorized-key flags are selected, raw
argument secrets cannot enter the report, and unexpected errors emit no private
input. It prints service health, four known public-client yes/no comparisons and
the running program's digest, never raw arguments, process environment, signing
credential files, journals or pairing capabilities. It modifies nothing. The
operator was asked to copy/run it on the household and return its JSON because
household administrator access is not configured from this VPS. That result is
now supplied, as recorded below.

## Running-client result and connection diagnostic

The operator returned all four configured public clients present in the selected
stable process arguments, zero malformed flags, active/running, zero restarts and
the expected executable digest. A missing restart is no longer a supported
hypothesis. No key regeneration or additional restart is requested.

Checker source `978063fcac146579852217d22abb67fb730cd7e9` adds private-pipe
diagnostics and an existing receive-client control. All ten workflows passed,
including [Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42869089242),
[CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42869089217),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42869089173)
and [real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42869089262).
Eight focused helper tests passed locally. A subsequent fixed-label addition
distinguishes a decrypted RPC refusal without exposing arbitrary error text.

Protected helper bytes and temporary units were independently hashed and verified
before execution. Sequential Primal checks under separate non-admin users loaded
only each user's own client credential. Both the payout client and existing
receive control returned unauthorized with `nak_client_key_used_matches=true`.
That checks the actual key passed into nak's NIP-46 connector. Membership was not
retried. A fresh payout diagnostic then returned `failed to connect` and
`response error: unauthorized`. The pinned dependency's connector issues
`connect` first and formats decrypted RPC refusals as `response error: ...`;
this identifies a bunker RPC refusal, without proving which signer instance
answered. No required kind was verified.

Only fixed labels, booleans and completed-kind numbers leave private pipes. Nak's
verbose authentication key never reaches a terminal, journal or retained report.
No signed challenge bytes are retained. The receive control additionally hides
the running receive service's credentials. No invoice/provider operation or
project publication occurred.

No nak bunker process runs on this VPS. The operator ran the extended household
inventory from `/home/linuxuser/lightning-goats/inspect-household-bunkers.sh`:
exactly one bunker process, matching the selected service, expected digest and
all four public-client arguments; no malformed flags or vanished processes.
This rules out an additional household process observed by that inventory,
not another host answering as the same identity. Four inspector tests pass, including
multiple-process filtering and secrecy. It reports only candidate counts,
known-public-client comparisons, executable hashes and whether a candidate is
the selected service; it reads no process environment or credential file. A
competing remote instance remains a hypothesis, not an established cause.

All three diagnostic processes were terminal. Their exact temporary unit files
were removed after byte comparison; private sanitized evidence/helper generations
remain. Original role journal hashes, configurations and all four original
service states match before/after. Receive health/status both return 200.
Production CyberHerd units stay masked/inactive with zero limits.

A synthetic four-client trial is added, pending CI. Using pinned nak, public test
scalars and a fresh loopback-only namespace, it must verify kinds 1/1311/9734 for
each of four clients configured through repeated flags, reject an unknown fifth
client by bunker RPC, retain authorized access and find no project publication.
It does not qualify the household executable or external relay behavior.
