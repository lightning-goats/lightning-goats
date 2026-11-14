# Separate CyberHerd NIP-46 authentication clients

`prepare-cyberherd-nostr-clients.py` prepares two separate encrypted authentication
credentials using the existing pinned `nak` client factory. Two clients are now
prepared as encrypted data, as recorded in the
[live preparation checkpoint](../testing/cyberherd-live-core-upgrade-20261110.md).
Both now pass [actual household round trips](../testing/cyberherd-bunker-checkpoint-20261110.md)
for membership 1/1311 and payout-signing 9734 after the operator stopped a competing
streaming-laptop bunker. No new production application service runs. The factory
never creates/imports the project's signing secret, contacts a bunker, signs
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
`authorize-household-nostr-client.py PUBLIC_CLIENT_HEX [PUBLIC_CLIENT_HEX ...]`
command remains the household-side helper. Multiple clients update one retained
allowlist and restart the bunker once; every key is validated before the backup
or replacement. Prepare concrete public-key commands after generation,
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

## Signing-only qualification

`deploy/scripts/check-cyberherd-nostr.py` is a non-admin checker, separate from
application workers and the root tool closure. Its two one-shot unit examples
are `cyberherd-nostr-membership-check.service.example` and
`cyberherd-nostr-payout-check.service.example`. Install only independently reviewed
helper bytes into a fresh root-owned protected prefix; substitute its exact path
and the actual public client in copies of the reviewed templates. Keep unique
temporary unit names, retain their exact bytes/hashes, and verify the assembled
units before starting. Never run the helper as root or enable it at boot.

Each unit loads only its own encrypted NIP-46 credential, uses its actual non-admin
identity and a private temporary runtime directory, and hides all application
journals, configurations, financial credentials and runtime IPC. Production units
stay masked and unchanged. The qualified existing Primal transport is the default;
additional approved relays are explicit bounded options, not assumed successes.

Membership signs exactly kinds 1 and 1311; the payout-signing client signs exactly
9734. Each uses a fresh fixed-purpose challenge, exact timestamp/content/tags,
expected project author and pinned client identity. Chat references a synthetic
unpublished activity; the zap request uses a publicly known synthetic recipient.
No recipient is contacted. Canonical IDs and pinned nak signature verification
must both pass. Child input/output/time are bounded, unrelated environment is
cleared and raw child output is withheld on error. Signed challenge bytes are
never printed or retained.

The only nak commands are public-client derivation, signing without publication
relay arguments, and local signature verification. NIP-46 transport can exchange
its encrypted envelopes; no project note/chat/zap request is published, no invoice
is requested and no provider/feeder operation is performed. The unsigned challenge
shapes follow [NIP-53](https://github.com/nostr-protocol/nips/blob/master/53.md) and
[NIP-57](https://github.com/nostr-protocol/nips/blob/master/57.md).

Retain only each sanitized success report and exact helper/unit identities in
private operator evidence. Stop/remove only the exact temporary unit copies;
leave changed files for inspection. A timeout or unauthorized response is a failed
round trip, not evidence that credential bytes should be regenerated. No result
enables publication or spending. Six local tests cover frozen-byte refusal,
fixed purposes, child time/output bounds, root refusal, unit isolation and fake
helper orchestration. They do not claim actual cryptography or household success.

The [actual check checkpoint](../testing/cyberherd-bunker-checkpoint-20261110.md)
records the earlier refusals/timeouts, inspection and eventual successful checks
after the operator found/stopped a competing streaming-laptop bunker. Membership
1/1311 and payout 9734 preserve the exact author, frozen unsigned fields, hash and
valid signature. A final connect observer saw one acknowledgment and no conflict.
No credential was regenerated and no project event, invoice or payment was sent.
The separate [native receipt check](../testing/cyberherd-zap-receipt-checkpoint-20261110.md)
now also passes 9735 using the existing core client and a synthetic invoice.
Live incoming settlement and new CyberHerd/receipt public delivery remain separate.

The optional `--diagnostics` mode adds only fixed stage labels, completed-kind
numbers, recognized error labels and a client-key-used match boolean. Pinned nak
verbose output contains the authentication key: the helper captures it only in
bounded private pipes, compares the selected key in memory and discards the raw
output. It never forwards or persists it. Do not run verbose nak directly into a
terminal/journal. An optional `receive` role tests kind 1 under the existing core
user with that user's own credential; it never gives the core client key to a
CyberHerd user. This control distinguishes a general signer/transport failure
from new-client behavior and publishes nothing. Eight focused tests cover the
diagnostic secrecy and bounded child execution alongside existing checks.

The payout-only `--observe-connection` mode subscribes to ephemeral NIP-46
responses before one fixed signing check. It uses a separate temporary nak
configuration and no credential environment for its read-only subscription.
Local decryption uses only that same payout-signing client's authentication key;
each response's signature/author/recipient is checked first. Only connect RPCs
contribute grouped acknowledgment/unauthorized/conflict counts. No response ID,
ciphertext, plaintext or signed event is printed or retained. Observation lasts
60 seconds, with at most 32 events and 64 KiB output. A missing/failed observer
is reported as such, never as proof that another signer does not exist. Use a
temporary check unit with 256 MiB memory allowance; production units stay masked.
Ten focused tests cover bounds, field secrecy and conflicting-response grouping;
the isolated real-nak fixture separately verifies actual local decryption and
observation. This mode neither publishes project events nor makes provider calls.
