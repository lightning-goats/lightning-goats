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

Constrained wallet reconciliation, held-policy updates, coordinated restore and
installed operational acceptance remain separate required work.


## Original acquisition ownership check

`lightning-goats-cashud reconcile-acquisition CONFIG APPROVAL` checks exactly one
retained acquisition under the exclusive wallet writer lock. Its native and
actual CLI checks pass in the checkpoint above. It starts no listener,
receipt-delivery worker or background scheduler. The protected approval binds the exact configuration bytes, wallet
instance/epoch, mint and acquisition UUID, with explicit review and a current
window no longer than 24 hours. It accepts no invoice, destination, replacement
request, seed or replay/clear-hold option.

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
  "approved_at": 0,
  "expires_at": 0
}
```

This template is intentionally invalid. Run as the configured non-admin wallet
UID with protected root-owned approval/configuration and matching existing private
keys/state. Normal startup registration, namespace and restore fences still apply.
An active wallet process retains its exclusive lock and prevents this command.

Only `acquiring` work and idempotent already-`acquired` retries are accepted.
For acquiring work, reconciliation derives the original output identities,
requires their existing CDK proof records and queries fresh mint states through
the pinned transport. Existing cryptographic and conservation checks must prove
ownership before the journal transitions to `acquired`. Missing recovery/output
material or unavailable mint state remains pending; invalid ownership remains
held. No swap, melt, quote, original-saga replay or receipt export is authorized.
The separately controlled receipt worker can later import a proven acquisition.

Validated/new, prepared, explicitly held, exported and credited operations are
refused. This command cannot clear `CASHU_RECOVERY_REQUIRED`, remove proof/source
claims, reset counters or reconcile an incoherent restored bundle. Those broader
recovery and held-work procedures remain required work. Private `status` already
prioritizes held acquisitions and reports their coarse causes without proof data.

The actual CLI fixture uses a disposable unfunded retained acquisition with no
output derivation, so its successful result is truthfully pending. It checks
exact approval scope, failed permissions/configuration, replay, active-writer
and restore fences and refusal to touch other acquisition states. Separate real
CDK tests cover owned-output reconciliation with all acquisition/replay/export
permissions disabled.

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
