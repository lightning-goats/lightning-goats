# Native incoming zap receipt signing qualification

Membership 1/1311 and outgoing 9734 already pass actual household signing;
see the [bunker checkpoint](../testing/cyberherd-bunker-checkpoint-20261110.md).
Incoming kind 9735 remains a separate check. Nothing here activates incoming
zaps, publishes a receipt or authorizes a payment.

`examples/check_household_zap_receipt.rs` calls the actual
`NakClient::sign_zap_receipt`, native frozen-field/hash/signature verifier and
pinned nak verifier. Its sole accepted argument is
`--synthetic-receipt-signing-only`. It uses the existing core authentication
client and expected project author over Primal; it never loads a project signing
secret or a financial credential.

The fixed request is the independently signed public fixture in
`tests/fixtures/nip57-nak-request.json`, preserving its exact Unicode/whitespace
bytes. The synthetic mainnet invoice uses a public test signing key, exact
10,000-msat amount, that request's description hash and a fresh random payment
hash. It is constructed locally, never issued by Strike or submitted for payment.
The job has no real settlement/store identity. The receipt is discarded after
verification; no invoice, receipt bytes or event ID is printed or persisted.

The helper requires a non-root single process identity, private canonical runtime
directory, exactly one bounded authentication credential, its expected derived
client public key and root-protected checksum-pinned nak. Invalid configuration
fails before remote signing. Underlying helper/filesystem errors never reach its
output. It opens no application config, ledger or API; calls no provider,
LNURL callback, publisher or feeder; and modifies only its private nak runtime.

The [one-shot unit](../../deploy/systemd/lightning-goats-zap-receipt-check.service.example)
runs as the existing core user, hides that running service's credential directory
and all production journals/configuration/IPC, and loads only its encrypted client
authentication credential. No financial secret is loaded. It uses no capabilities,
strict filesystem and private-network restrictions, 256 MiB memory and no core
dumps. Never enable it at boot or replace/restart the live receive service for
this check.

After checks pass on exact source, download the matching test-only artifact,
verify its outer digest, internal checksums and source identity, then select
`SIGNING-ONLY-check-zap-receipt`. Install that exact executable into a fresh
root-owned protected prefix. Substitute its path into one uniquely named
temporary unit copy; hash and verify the assembled unit before starting it once.
Preserve protected before/after role-journal hashes/configurations/service states.
Retain only its sanitized report and exact executable/unit identities in private
operator evidence. Remove only the exact terminal temporary unit, retaining a
changed file for inspection. No credential rotation or household restart is part
of this procedure.

The CI real-nak fixture additionally signs this job through the actual native
client/bunker and verifies no kind-9735 event appeared on its isolated relay.
An offline example test validates the synthetic invoice/request/store boundary.
These checks do not establish live incoming settlement, receipt durability or
public delivery. Existing native receive regressions cover those paths with
mocked Strike; actual activation/controlled settlement remains separate.

Current status: source prepared; exact-source CI/artifact and actual-host signing
qualification pending. Do not claim a live receipt or new incoming-zap support
from the existence of the helper.
