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

The operator was asked to restart only the existing household bunker to reload
its retained allowlist. A stale running allowlist is a hypothesis, not a proven
cause. No new client/key generation or household setting change is needed or
authorized by the failure itself. After confirmed reload, retry the bounded
checks, require exact author/bytes/hash/signature for every expected kind, and
retain the result before any publication or financial activation. If refusal
persists, inspect only public running-client identities rather than dumping
raw process arguments, environments, bunker logs or pairing capabilities.
