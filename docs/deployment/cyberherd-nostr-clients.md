# Separate CyberHerd NIP-46 authentication clients

`prepare-cyberherd-nostr-clients.py` prepares two separate encrypted authentication
credentials using the existing pinned `nak` client factory. This is prepared
source, not a claim that new clients exist or are authorized at the household.
It never creates/imports the project's signing secret, contacts a bunker, signs
an event, publishes or changes a service. Keep the existing receive client intact.

After reviewing the complete [authenticated tool bundle](inactive-upgrade-transaction.md),
select the **actual public** project and existing core-client identities:

```sh
sudo /usr/bin/python3 -I -S -B /ROOT_OWNED_BUNDLE/launch-vps-upgrade.py \
  TOOL_MANIFEST_SHA256 --cyberherd-nostr-clients \
  --project-pubkey PROJECT_PUBLIC_HEX \
  --core-client-pubkey EXISTING_CORE_CLIENT_PUBLIC_HEX --create-client-keys
```

Only public keys are command arguments. New private authentication material
passes between pinned nak and systemd-creds through pipes; it is never printed
or put into arguments. The host key, credential store and installed nak must
already be protected; an unexpected nak digest fails before key creation.
The wrapper checks canonical root ownership/permissions and refuses clients
matching the project, core client or each other. It never rotates a conflicting
existing credential automatically. Existing valid credentials are reused exactly.

The two destinations are root-owned 0600:

- `/etc/credstore.encrypted/cyberherd-nostr-client`, for optional membership
  note/chat publishing, kinds 1 and 1311.
- `/etc/credstore.encrypted/cyberherd-signer-client`, for the purpose-limited
  outgoing 9734 broker; it has no relay publication capability.

Public JSON output includes the role, derived client public key, encrypted path
and creation status, with zero events and no household/service change. If a later
role fails, retain earlier output files; retry reuses them rather than generating
another identity. Temporary caches are private and task-owned. SELinux labeling
is applied when SELinux is present; directory publication is synced.

The household administrator must separately add both actual public keys while
retaining existing authorized clients. The existing
`authorize-household-nostr-client.py PUBLIC_CLIENT_HEX` command remains the
household-side helper; prepare concrete public-key commands after generation,
rather than authorizing placeholder identities. No SSH administrator access or
new household port is assumed. Adding an allowlist entry does not itself prove
actual transport, author, purpose/kind permissions or public relay delivery.

Use each public key in its own disabled protected configuration. Membership's
unit example uses optional `LoadCredentialEncrypted=nostr-client-key` with its
new path; the broker already uses its separate path. Do not give either client
credential to the payout worker or reuse the receive daemon's credential.
Household nak/NIP-46 round trips for kinds 1/1311/9734, exact author/unsigned bytes,
wrong-purpose refusal and actual relay acknowledgments still require qualification
before enabling their respective publication/execution paths. Local synthetic
factory tests prove namespaces/retention, not household authorization.
