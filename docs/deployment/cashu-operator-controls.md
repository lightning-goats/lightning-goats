# Protected Cashu settlement planning

`lightning-goatsctl plan-cashu-settlement` previews or retains one bounded
operator request through the existing source-allocation and treasury planner.
It accepts no invoice, payment hash, destination, key or provider credential.
The destination and all financial limits come from the exact protected core
configuration approved for this operation.

The [source-pinned checkpoint](../testing/cashu-operator-controls-20261117.json)
passes the native gates and actual isolated CLI fixtures. The
[eight financial composition cases](../testing/cashu-operator-composition-20261117.json)
also pass on matching core/wallet artifacts. Production installation and financial
activation still require their separately approved deployment steps.

Run as the non-admin **core ledger owner**, with a root-owned non-writable
configuration and approval. All their parent directories must also be root-owned
and protected. Stop the core through the separately approved maintenance process;
the command only inspects systemd and refuses an active/unknown service. It never
stops, starts or enables a unit itself.

An approval is bounded JSON with no unknown fields:

```json
{
  "version": 1,
  "core_epoch": "33333333-3333-4333-8333-333333333333",
  "wallet_instance": "11111111-1111-4111-8111-111111111111",
  "wallet_epoch": "22222222-2222-4222-8222-222222222222",
  "config_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
  "request": {
    "id": "44444444-4444-4444-8444-444444444444",
    "mint_id": "operator-selected-configured-mint",
    "maximum_input_sats": 200
  },
  "reviewed_settlement_planning": false,
  "approved_at": 0,
  "expires_at": 0
}
```

This template is intentionally invalid. Bind the real configured identities,
SHA-256 of the exact configuration bytes, an approved mint and bounded input.
The root reviewer must explicitly approve planning and a current validity window
of at most 24 hours. Comments or other byte changes to configuration invalidate
the hash binding. The command hashes and parses the same bounded bytes, without
reopening a different configuration image.

Preview is the default:

```sh
sudo -u lightning-goats /usr/local/bin/lightning-goatsctl \
  --config /etc/lightning-goats/config.toml plan-cashu-settlement \
  --approval /etc/lightning-goats/approvals/cashu-plan.json --writers-stopped
```

The planner executes source selection inside a transaction and rolls it back.
No request row, custody reservation, contribution, event or feeder attempt is
retained. Preview displays prospective amounts, not a persisted intent ID.
Adding `--apply` commits that same planning path. This command opens only an
existing recovery-validated ledger; it does not initialize or migrate a database
or create a missing core epoch.

Use the same request UUID for retries. Its core/wallet binding, mint and input
cap cannot change. When already assigned, retries return the original verified
intent rather than creating another reservation. If no eligible custody exists,
apply retains the exact request binding but creates no intent. It then needs a
later explicit retry with a current approval; this does not add a new automatic
sweep trigger to configuration. The shared planner still enforces source holds,
per-mint/global caps, fees, daily budgets and unresolved exposure.

Planning sends no provider request and performs no physical actuation. An
applied intent can later be processed by the existing independently controlled
treasury workers. Their invoice/destination verification, wallet authority,
independent Strike settlement and physical-feeder gates remain mandatory.

Qualify funded preview/replay/epoch behavior with native ledger tests, then run
the actual CLI fixture under fresh mount/network namespaces:

```sh
sudo unshare --mount --net -- python3 deploy/scripts/test-cashu-private-run.py -- \
  python3 deploy/scripts/test-cashu-operator-settlement-cli.py \
  --cli /reviewed/path/lightning-goatsctl --cli-sha256 REVIEWED_SHA256
```

The CLI fixture uses unused UIDs, native migrations, an empty synthetic ledger
and a namespace-local mocked systemctl response. It checks preview rollback,
active-service refusal, configuration/approval scope and permissions, one-time
request retention and conflicting retries. It does not qualify installed
systemd, mint/provider behavior or funded custody; native funded tests and
existing financial composition provide those separate synthetic boundaries.

Held-policy updates, coordinated restore and installed operational acceptance
remain separate required work.

## Original treasury melt reconciliation

Use `lightning-goatsctl inspect-cashu-settlement --intent INTENT_UUID` as the
core ledger owner to inspect the same intent shown in private wallet status.
The command opens the configured private ledger read-only and works while the
core runs. It reports the retained Strike receive identifiers, verified deposit
time, wallet report presence, settlement totals and retry schedule. A linked feeding
plan reports whether its exact pending allocation is covered, or the original
feeder request and its retained status if already dispatched. A covered plan
still goes through the separate feeder readiness and dispatch checks.

The output contains no invoice, payment hash, credential or proof material.
It reads a single journal snapshot and contacts neither Strike nor the wallet.
The installed recovery fence still governs access to a restored ledger.

Core verifies the original Strike receive against the retained treasury intent,
destination and signed invoice. A single authoritative receive-request lookup
must confirm the BTC target. The completed receive must match the original
Lightning invoice, payment hash and amount in BTC before core records it.

Run `lightning-goatsctl reconcile-cashu-settlement` as the registered core
runtime user while the core service is stopped. Use the established
receive-only Strike credential delivery for a fresh verification. The command
matches that credential to the frozen destination fingerprint. Use the wallet's
private treasury status socket to retrieve a report still missing from core,
with new treasury admission and execution paused.
The same command can record the verified Strike deposit while the wallet status
is unavailable. Once the wallet can return the original report, rerun with
the same approval to finish. Core uses its existing transactional settlement
path when both records agree.

```sh
lightning-goatsctl --config CORE_CONFIG reconcile-cashu-settlement \
  --approval APPROVAL --writers-stopped
lightning-goatsctl --config CORE_CONFIG reconcile-cashu-settlement \
  --approval APPROVAL --writers-stopped --apply
```

The first invocation previews the evidence without writing financial records.
Both invocations require a root-owned approval with these fields:

- `version` is `1` and `purpose` is `"reconcile_original_cashu_settlement"`.
- `core_epoch`, `wallet_instance`, `wallet_epoch`, `intent_id`,
  `receive_request_id` and `receive_id` identify the retained operation and original
  Strike receipt.
- `intent_digest` is the original core intent digest. `config_sha256` is the
  SHA256 of the exact core configuration. Use lowercase hex for both.
- `approved_at` and `expires_at` bound the approval in Unix seconds, with a
  maximum interval of 24 hours.

Use the retained `cashu_treasury_intents.intent_digest` and its matching
`cashu_treasury_issuances.receive_request_id`. The Strike receipt's
`receive_id` identifies the completed receive under that request. Confirm
that pair in private Strike evidence before creating the approval.

The private result reports which records are retained and whether core completed
the settlement. The command reuses a matching retained deposit or wallet report
without contacting that source again. It uses the existing live ledger's
current schema and requires restored ledgers to remain behind their recovery
fences. Feeding remains the responsibility of the normal feeder workflow.

`lightning-goats-cashud reconcile-treasury CONFIG APPROVAL` checks one retained
submitted melt under the wallet's exclusive writer lock. Run it under the
registered wallet UID with the wallet stopped. It uses the same original quote
lookup, preimage validation and change ownership checks as the wallet's recovery
worker. An already retained report can be returned from the journal without a
mint request. A bound instruction with no recorded submission is refused.

The root-owned reconciliation approval JSON contains these fields:

- `version` is `1`, and `purpose` is `"reconcile_original_treasury_melt"`.
- `wallet_instance`, `wallet_epoch`, `core_epoch` and `intent_id` are UUIDs
  from private diagnostics for the original operation.
- `mint_id` identifies its configured mint, and `config_sha256` is the
  lowercase SHA256 of the exact wallet configuration.
- `approved_at` and `expires_at` bound the approval in Unix seconds, with a
  maximum interval of 24 hours.

An unpaid or pending quote produces `pending`. A proven paid quote produces
`report_recorded`, and an exact retry produces `report_already_retained`.
Both report outcomes include the original `operation_evidence_digest`. Transport
and evidence errors retain the original operation for investigation.

Use `lightning-goats-cashud cleanup-treasury CONFIG APPROVAL` for the CDK
cleanup stage. Create a separate approval with the same wallet, core, config,
mint and intent bindings. Set `purpose` to `"cleanup_original_treasury_melt"`
and include the report's exact lowercase `operation_evidence_digest`. The
command requires the original completed report and fresh mint evidence that
every corresponding input is spent. It removes those inputs and the original
CDK saga, records the paid quote, and checkpoints cleanup. An exact retry
returns `cleanup_already_retained` from the journal without another mint call.
An unavailable or incomplete proof-state read preserves the original work.

These commands perform wallet recovery. Core must independently verify the
original Strike receive before crediting it. The commands run with the wallet
stopped, leave restored wallets fenced, and access the mint only for original
operation evidence. The funded lost-reply fixture covers both stages,
including refusal of mismatched, missing and incomplete evidence. It also
interrupts cleanup after the CDK changes and verifies that the original
operation can finish on retry. Installed acceptance remains open.


## Original acquisition ownership check

`lightning-goats-cashud reconcile-acquisition CONFIG APPROVAL` checks exactly one
retained acquisition under the exclusive wallet writer lock. Its native and
actual CLI checks pass in the checkpoint above. It starts no listener,
receipt-delivery worker or background scheduler. The protected approval binds the exact configuration bytes, wallet
instance/epoch, mint and acquisition UUID, with explicit review and a current
window no longer than 24 hours. The command accepts only original-operation identifiers
and the approval fields below.

For a provisioned, separately approved installation:

```sh
sudo -u lightning-goats-cashu /usr/local/bin/lightning-goats-cashud \
  reconcile-acquisition /etc/lightning-goats-cashu/config.json \
  /etc/lightning-goats-cashu/approvals/ownership.json
```

Both files and all their ancestors must be root-owned and non-writable by the
runtime identity. The approval template above is for core planning; ownership
checks use the distinct schema below, which is also intentionally invalid.

```json
{
  "version": 1,
  "wallet_instance": "11111111-1111-4111-8111-111111111111",
  "wallet_epoch": "22222222-2222-4222-8222-222222222222",
  "acquisition_id": "33333333-3333-4333-8333-333333333333",
  "mint_id": "operator-selected-configured-mint",
  "config_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
  "reviewed_ownership_check": false,
  "reviewed_held_ownership_check": false,
  "approved_at": 0,
  "expires_at": 0
}
```

This template is intentionally invalid. Run as the configured non-admin wallet
UID with protected root-owned approval/configuration and matching existing private
keys/state. Normal startup registration, namespace and restore fences still apply.
An active wallet process retains its exclusive lock and prevents this command.

The default scope accepts `acquiring` work and idempotent already-`acquired` retries.
For acquiring work, reconciliation derives the original output identities,
requires their existing CDK proof records and queries fresh mint states through
the pinned transport. Existing cryptographic and conservation checks must prove
ownership before the journal transitions to `acquired`. Missing recovery/output
material or unavailable mint state remains pending; invalid ownership remains
held. No swap, melt, quote, original-saga replay or receipt export is authorized.
The separately controlled receipt worker can later import a proven acquisition.

Set `reviewed_held_ownership_check` to `true` to recheck a hold raised while
the original acquisition was `acquiring`. Set `held_reason` to the exact
reason from private wallet status. Supported reasons are
`missing_owned_outputs`, `ambiguous_acquisition`, `invalid_mint_response`
and `conservation_mismatch`. Existing approvals without `held_reason` remain
scoped to `missing_owned_outputs`.
This scope retains the hold until the original signed outputs pass the full
ownership, attribution and conservation checks. On success, one journal
transaction records the output attribution and advances that acquisition to
`acquired`. The original hold reason and request remain in the journal.
The same approval can be retried while the recovered acquisition is `acquired`.
Holds raised before submission or after acquisition and operations already
exported or credited require separate reconciliation. A hold recorded before
submission retains its proof claims and stays held. Later disjoint payments
at that mint can proceed when the journal has no evidence of submission for
the held operation and the mint-scoped CDK store has no unfinished saga.
The acquisition worker preserves the retry budget of payments waiting on
that local recovery check. It can continue eligible work at another configured
mint in the same tick. A fresh attempt is recorded durably before the mint
request, with the existing custody limits checked again. Earlier recorded
attempts retain their original counts.
Keep `CASHU_RECOVERY_REQUIRED` in place on restored state.
Private `status` prioritizes active holds and shows their coarse causes.

The actual CLI fixture uses a disposable unfunded retained acquisition with no
output derivation, so its successful result is truthfully pending. It checks
exact approval scope, failed permissions/configuration, replay, active-writer
and restore fences and refusal to touch other acquisition states. The fixture
also rechecks eligible holds without enough evidence and leaves them unchanged.
A funded CDK fixture drops part of a state response, restarts while the
acquisition is held, and rechecks the original outputs before the receipt worker
exports once. The mint executes one swap throughout.

## Mint policy review

Create a root-owned protected candidate from the current wallet configuration.
Use stable internal mint IDs and exact advertised URLs. To retire a mint from
receiving, set its `receive_cashu` and `receive_nutzap` flags to false. Keep its
configured identity, namespace and pinned keysets for retained custody and
original operations. Existing mint URLs and previously pinned denomination
keys must survive the update. An issuer retains its ID even when its namespace
is absent from the current config. The wallet refuses a new ID for that URL.

Run the review as the stopped wallet's runtime user:

```sh
sudo -u lightning-goats-cashu /usr/local/bin/lightning-goats-cashud \
  review-mint-policy /etc/lightning-goats-cashu/config.json \
  /etc/lightning-goats-cashu/mint-candidate.json
```

The reviewer holds the existing writer lock, checks the recovery fence and the
wallet's key and epoch registration, and reads the journal without running
migrations. It reports the exact config hashes, proposed rail flags, retained
namespace status, uncredited acquisitions and open treasury instructions for
each candidate mint. Review those counts alongside the wallet's private status
for current custody. The receipt describes a candidate, with `applied:false`.

The mint policy review permits changing mint and keyset inventories and their
descriptor IDs. Use the current request endpoint and move a replaced QR to a
reviewed retirement window, or revoke it. The candidate can pause dependent
receive and publication switches. Existing spend, signer, wallet registration
and broader runtime settings must remain identical. Review core receipt-policy
acceptance before enabling admission under a new mint policy version.

After operator approval, use the [authenticated maintenance capsule](cashu-backup-collection.md)
and its reviewed policy to install the exact candidate. Keep a separate
protected copy of the original config with its ownership, mode, extended
attributes and bytes preserved. The original copy and candidate both need to
be readable by the wallet runtime identity. The policy specifies the wallet
config destination, runtime identity, service unit and binary digest.

```sh
/usr/bin/python3 -I -S -B ROOT_CAPSULE/launch-cashu-backup.py TOOL_SHA256 \
  apply-mint-policy POLICY POLICY_SHA256 BASELINE BASELINE_SHA256 CANDIDATE CANDIDATE_SHA256
```

The installer verifies the stopped and masked wallet unit and an idle runtime
UID. It reruns the native review in a private network namespace and takes the
host maintenance and wallet writer locks before publishing the file. The
installed candidate inherits the original config's metadata. After an
interruption, rerun with the same protected inputs to verify the exact
candidate or finish an interrupted install. The installer rejects changed
installed bytes or metadata, an active writer, and a recovery fence. It
reports installation without starting or unmasking a service. For a later
correction, review a new candidate from the installed config so any subsequent
work remains recoverable.

The local fixture exercises the authenticated installer and an exact retry
with synthetic service status, real runtime UID checks, and file locks.
Hosted CI also runs the native wallet in a disposable systemd unit with a
separate UID and private networking. The real installer must refuse the
active unit and the unmasked inactive unit, then apply and verify an exact
retry under the actual mask. The fixture separately verifies that startup
is still blocked by the mask. After the fixture removes its own mask, the
native wallet restarts with the new mint namespace and preserves the original
held acquisition. Both fixtures use synthetic, unfunded state.

```sh
sudo unshare --mount --net -- python3 deploy/scripts/test-cashu-private-run.py -- \
  python3 deploy/scripts/test-cashu-operator-reconciliation-cli.py \
  --cli /reviewed/path/lightning-goats-cashud --cli-sha256 REVIEWED_SHA256
```


```json
{
  "version": 1,
  "wallet_instance": "11111111-1111-4111-8111-111111111111",
  "wallet_epoch": "22222222-2222-4222-8222-222222222222",
  "acquisition_id": "33333333-3333-4333-8333-333333333333",
  "mint_id": "operator-selected-configured-mint",
  "config_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
  "reviewed_ownership_check": false,
  "approved_at": 0,
  "expires_at": 0
}
```

Only an `acquiring` original may newly transition to `acquired`. It must have
retained derivation material, the exact expected CDK-owned outputs, pinned mint
keys and fresh authoritative states passing the existing cryptographic and
conservation checks. An already `acquired` original returns its retained result
without creating an export. Missing material or a timeout stays pending; invalid
ownership can retain a hold. New, prepared, held, exported and credited states
are refused. This is not whole-wallet restoration or held-operation release.
An existing restore marker still blocks startup, and an active wallet writer
keeps its exclusive lease. No lease is stolen or expired.


The wallet CLI fixture compares SQL schema definitions and all retained rows,
including rowids used for queue order. Normal wallet startup recreates identical
treasury views, so schema-object insertion order is excluded from that comparison.
Its unfunded pending operation never contacts a mint. Funded output verification
is qualified separately by the native CDK regression; installed provider,
whole-wallet recovery and held-operation release remain open.
