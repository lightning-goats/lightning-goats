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

## Remaining connection and activation

After the operator runs the household script, read back its public relay list
and service state. Match the VPS relay list, load only the client credential in
the payment service and obtain/verify one remote kind-1 signature without
publishing it. Confirm the expected author/content/tags and actual service-user
access. Do not print bunker output, pairing URLs or process environments.

The daemon is still `canary`; public Nostr remains disabled and its outbox is
empty. Public messaging activation remains a separate operator step. Before
switching to `active`, preserve the paid ledger, verify no unresolved feed, and
check the message cursor is caught up so earlier test activity is not published.
Keep the existing confirmed-feeding behavior, 10-sat announcement threshold,
overlay-only informational/weather audiences and exact signed-event retries.
No Lightning payment, added credit or physical feeding is needed for the
signing-only connection check.
