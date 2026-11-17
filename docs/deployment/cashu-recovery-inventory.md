# Cashu coordinated recovery inventory

Development contract for implementation-plan section 13. This is an inventory
and reconciliation contract; coordinated backup/restore tooling and destructive
rollback fixtures remain incomplete. No production maintenance is authorized.

The optional `acquisition_admission_times` table is observational metadata inside
the complete wallet journal. New records commit with admission; older records
may legitimately lack a timestamp. Missing timing metadata means unknown age,
never permission to replace financial identities, retry a new operation, infer
settlement or release a recovery fence. Status reads do not migrate old stores.

A recovery set must identify one quiesced checkpoint across these components:

| Component | Required retained material | Why it is required |
| --- | --- | --- |
| Core ledger | Complete SQLite database, ledger epoch, Cashu receipt inbox, grants/retirements, financial plans, source allocations, treasury jobs/results/deposits, feeder requests/results and signed outboxes | Prevent duplicate contributions, repeated deposits, reward grants and physical dispatch. |
| Wallet journal | Complete `acquisition.sqlite`, wallet epoch/instance registration, admitted input identities, prepared request bytes, acquired evidence, acknowledgments, schedules, treasury reservations/results/change and publication outboxes | Recover exact operations and ownership; an input marked spent does not prove output ownership. |
| CDK wallet | Complete legacy `wallet.sqlite` plus every retained `mint-<sha256>.sqlite` namespace, with their operation/counter/proof state at the same checkpoint | Retain wallet-owned outputs and deterministic preparation state without reverting proof reservations. |
| Wallet keys | Protected `wallet-seed.bin` (64 raw bytes), `receiving-key.bin` (32 raw bytes), and retained historical receiving keys with policy bindings | Seed recovery alone cannot restore pending P2PK inputs, original receipt attribution or core decisions. Never generate replacement keys during restore. |
| Signing broker | Complete `cashu-signing.sqlite`, broker epoch, wallet binding and exact signed-event records | Preserve signing authorization and exact-event retry identity. |
| CyberHerd membership | Complete membership journal, owner/core cursor bindings, retained roster, reward plans and exact message outboxes | Preserve the existing four-journal correlations and avoid replaying membership/reward decisions. |
| CyberHerd payout | Complete payout journal, original provider/quote/payment identities, held costs and core acknowledgments | Reconcile original payments and reservations without issuing replacements. |
| CyberHerd payout signer | Complete payout-signing journal and immutable owner/client/peer policy | Keep payout signing separate from Cashu receiving announcements and preserve exact signed requests. |
| Scoped NIP-46 client authentication | Protected recovery material or a separately qualified operator reprovisioning procedure for each signer client, including Cashu and payout brokers | Restoring the signing journal alone does not restore authentication to the external signer. Preserve purpose and public-client bindings; the project signing key stays with its existing external signer. |
| Configuration and artifacts | Protected core/wallet/broker policies, mint IDs and exact URLs/keysets, descriptor versions, UID/socket bindings, source revision, binary hashes and schema versions | Bind recovery to the admitted historical policies and qualified programs. |
| External reconciliation evidence | Original mint operation IDs/requests, Strike receive IDs/payment hashes and feeder UUID/result evidence, retained privately with the checkpoint | Reconcile external effects that may have happened after the snapshot. |

Snapshot creation must first prevent new admission and quiesce every affected
writer, including core settlement/receipt/feeder workers, wallet/CDK workers and
the signing broker. Verify their exact service/process identities and obtain the
existing writer authority before copying. A live filesystem copy of a SQLite
main file alone is insufficient: use a qualified SQLite backup or a verified
quiesced database/sidecar set. Keep source databases and uncertain sidecars intact
until successful verification. Never delete a WAL to make an inventory appear
complete. Encrypt secret-bearing backup components and keep their contents out
of source control, logs and public manifests.

Verification must use disposable copies, check database integrity and schema,
compare epochs/registrations and cross-store immutable receipt/treasury digests,
and account for every pending operation, reservation and acknowledgment. Hashes
establish snapshot identity, not agreement with current mint or Strike state.
Inventory output should contain component identities and hashes without proof
bodies, raw keys, status capabilities or private source context.

Before opening a restored core, place the existing
`CYBERHERD_RECOVERY_REQUIRED` marker beside its ledger database. The shared durable
SQLite open guard checks that marker before creating/opening/migrating state.
Before opening a restored Cashu wallet or Cashu broker, place
`CASHU_RECOVERY_REQUIRED` in each of those restored state directories. Current startup refuses any entry with that name,
including a dangling symlink, before opening databases. This explicit fence does
not detect arbitrary rollback and must not be automatically cleared. Core receipt
import, new wallet acquisition, treasury spending and physical dispatch remain
fenced pending cross-store and external reconciliation. Do not invent a fresh
epoch, acquisition ID, treasury ID or feeder UUID to bypass retained uncertainty.

Required recovery fixtures must independently restore only the wallet, only the
core, and the coordinated set after later activity. Each must prove no second
credit, melt, reward or physical dispatch; unresolved outputs and side effects
remain held. Include lost key/component, interrupted snapshot, prepared swap,
lost swap reply, melt pending/paid, returned change, lost core acknowledgment and
signed-but-unacknowledged publication boundaries. Validate recovery from retained
historical policies after new acceptance for a mint/key is disabled.

An operator recovery decision may resume only work whose original identities,
ownership, external outcomes and cross-store accounting have been reconciled.
The explicit marker is currently a stop condition, not an implemented recovery
approval command. Existing maintenance tooling that refuses Cashu state must
continue to refuse until this coordinated workflow is implemented and qualified.

The offline `deploy/scripts/verify-cashu-backup.py <snapshot-directory>` checks a
private, current-user-owned canonical directory containing `inventory.json` and
only listed components. Inventory version 1 contains `checkpoint_id` (nonzero
UUID), `registrations` with canonical nonzero `core_epoch`, `wallet_instance`
and `wallet_epoch` UUIDs, and `components`, each with `role`, relative `path`, and `sha256`. Required
roles are `core_ledger`, `wallet_journal`, `cdk_wallet`, `signing_broker`,
`wallet_seed`, `receiving_key` and `protected_policy`; only policy may repeat.
Keys require their exact raw sizes. All files require mode 0600 or stricter,
single links and no aliases. Unlisted files, including database sidecars, refuse.
The verifier checks frozen database bytes in disposable copies with SQLite
integrity checks; it never opens source databases through SQLite. Its output
also compares the core overlay epoch and wallet registration with the inventory,
and requires the signer registration to name that wallet instance/epoch. It
does not authorize restore or establish writer quiescence, complete schema
compatibility, receipt/treasury acknowledgment agreement, external outcomes,
historical-key completeness or the completeness of policy
files. Those checks and coordinated snapshot creation remain required.

The native [copy-normalization helper](cashu-backup-collection.md) is being
qualified separately. It operates on protected raw copies with the wallet's
pinned SQLite engine; it supplies no writer-quiescence, encryption, coordinated
checkpoint or restore authority.

Matching registrations identify the expected stores; they do not detect an older
snapshot of the same epoch. Cross-store receipt/treasury digests and external
reconciliation are still required before any recovery fence can be cleared.

The verifier additionally checks each `credited` wallet acquisition's retained
core acknowledgment against the core receipt inbox, including registration,
acquisition ID, receipt digest, event sequence, net sats and original returned
pool-credit total. Missing or conflicting core history refuses the snapshot.
For the inventoried wallet instance/epoch and core epoch, the reverse check also
requires each core receipt to have matching acquired wallet history, net value
and acquisition-evidence digest. A `credit_export_pending` record without an
acknowledgment is allowed when that retained acquisition matches; verification
does not manufacture an acknowledgment. Other registrations, pending acquisitions,
treasury results, CDK ownership and external effects still require reconciliation. A
passing inventory remains insufficient authority to restart restored workers.

For the inventoried registration, every core-retained treasury wallet result must
also have an identical parsed report in the wallet snapshot. Missing parent
intents or missing/conflicting wallet reports refuse. This is a local historical
agreement check; it does not independently verify melt completion, mint proof
ownership, Strike settlement, fee/source conservation or pending instruction
recovery. Wallet-only reports may represent an interrupted delivery and are not
synthesized into the core by verification.

## Mint-scoped CDK layout

The collision correction retains `wallet.sqlite` as a legacy file and stores
new CDK custody in `mint-<sha256>.sqlite` files. The filename digest binds the
versioned `lightning-goats/cdk-mint-store/v1` domain and stable mint ID. The shared
journal retains `cdk_storage_layout` version 1 and `cdk_mint_namespaces` rows;
namespace files independently bind the mint ID and exact URL. All retained
namespaces belong in a coordinated backup, even when new acceptance is disabled
or the mint is absent from current configured work.

Inventory role `cdk_wallet` retains the legacy file. Add one `cdk_mint_wallet`
component for every journal namespace, with the ordinary `path`/`sha256` plus
`mint_id` and `mint_url`. This role may repeat; mint IDs and paths may not.
Verification compares the complete set with the journal, requires ready
namespaces and exact file bindings, and refuses nonempty legacy proof/saga
custody. These checks do not migrate an old shared store. A partial namespace or
legacy financial snapshot remains retained for explicit reconciliation even when
it cannot pass the ready-layout verifier. Never delete evidence to make it pass.

The offline inventory's filesystem walk also rejects special entries, including
unlisted FIFOs and Unix sockets, rather than ignoring them as non-files. Any
directory traversal error refuses verification, so unreadable paths cannot hide
undeclared components. The original FIFO regression failed before correction;
all sixteen inventory tests now pass
([evidence](../testing/cashu-backup-special-entries-20261116.json)). These checks
remain read-only and do not establish snapshot quiescence or authorize restore.

The new `deploy/scripts/test-cashu-native-backup.py` fixture is implemented
and syntax-checked, with native execution still pending. It selects and hashes
the actual core/wallet/signer CLIs, initializes disposable journals under three
different non-root UIDs, includes two actual mint-scoped CDK stores, takes
SQLite backups after child exit while holding wallet/signer writer locks, and
invokes the inventory verifier. It also tests missing-key/namespace and wrong-
epoch refusal, plus unchanged wallet bytes behind the explicit recovery marker.
CI now builds/transfers all three CLIs and invokes this fixture. This is an
empty-state schema/binding checkpoint; it does not qualify funded rollback or
coordinated restore, a generic production snapshot collector, external outcomes,
or fence clearance. See the
[preparation record](../testing/cashu-native-backup-preparation-20261116.json).

Component verification now hashes into private disposable copies using 1 MiB
chunks, rather than retaining a whole component in Python memory. The source
is opened with no-follow/nonblocking flags, checked as private regular state,
and checked for stable identity, size and timestamps during the copy. Frozen
bytes must match the inventory SHA-256 before SQLite checks run. A 32 MiB
component exceeded 35 MiB traced Python allocation before correction; the new
regression requires less than 16 MiB and passes. All seventeen inventory tests
pass ([evidence](../testing/cashu-backup-streaming-20261116.json)). This does not
establish writer quiescence or external reconciliation.

The actual-CLI empty-state fixture now passes on three selected, hash-pinned
native programs ([evidence](../testing/cashu-native-backup-result-20261116.json)).
It initialized real core, wallet and signer schemas under distinct UIDs,
verified eleven components including two CDK namespaces, rejected a missing
receiving key/mint store and wrong wallet epoch, and proved fenced wallet
startup leaves the retained bytes unchanged. Both CLI builds completed with
unchanged source hashes. This supersedes the pending status above for this
fixture only. Funded post-snapshot activity, wallet-only/core-only rollback,
coordinated restore and external ownership/outcome reconciliation remain open.

All three native fence boundaries now pass an actual-CLI fixture
([evidence](../testing/cashu-native-recovery-fences-20261116.json)). A valid
synthetic credential/configuration first starts the separate Cashu broker on
a disposable private socket and shuts it down cleanly, proving the startup
control is otherwise valid. Core, wallet and broker then reject their explicit
markers with database/key bytes unchanged. No signing-helper request, provider
contact or host service change occurs. This remains initialized empty state;
it does not qualify funded rollback or external reconciliation.

## Existing CyberHerd journals remain part of full recovery

The current Cashu inventory verifier and eleven-component native fixture are
a partial recovery-set check. They do not snapshot or reconcile the existing
CyberHerd membership, payout-authority and payout-signing journals. The full
operator workflow must extend the existing four-journal inventory with the
Cashu wallet, legacy/scoped CDK stores, separate Cashu signing broker and keys;
it must not replace that inventory with the partial Cashu set. Keep the two
signing roles distinct. Cross-store payout funding, consumed credit, exact
signed outboxes and unresolved financial/physical identities must all agree.
Existing maintenance tooling must continue to refuse Cashu state until the
combined inventory/migration/restore workflow and funded rollback cases are
implemented and qualified. No current verification result authorizes restart
of a restored production stack.

Frozen database verification now requires both `PRAGMA integrity_check` and
`PRAGMA foreign_key_check`, with query-only mode and trusted schema disabled.
An orphaned relationship previously passed the integrity check and the verifier;
the regression now refuses it and accepts the corresponding complete relationship.
All eighteen inventory tests and the native eleven-component/fence fixture pass
([evidence](../testing/cashu-backup-foreign-keys-20261116.json)). This strengthens
local component integrity; complete existing-CyberHerd correlation and funded
restore/external reconciliation remain required.

## Native four-journal correlation bridge

`cyberherd-statectl review-cashu-correlations --bundle PATH --destination PATH`
is a read-only subset review. It does not create a backup or restore staging.
The protected maintenance policy still binds all four journals and their native
core epoch, owner, provider, cursor, grant and signer relationships. Ordinary
four-journal snapshot/verify/restore continues to refuse Cashu custody.

The input is a private root-owned directory containing only main-only `core.db`,
`membership.db`, `payout.db`, `signer.db` and `MANIFEST.json`. Its distinct manifest
has version `2`, purpose `cashu_four_journal_correlation_review`, a nonzero UUID
`id`, the full protected `policy`, `source_policy_sha256`, a 64-hex
`cashu_inventory_sha256`, and four `images` with `name`, `bytes`, `sha256`.
An `INCOMPLETE` marker refuses review. The destination must be a new file outside
the input, in a private root-owned directory. Images and manifest are checked
again after native read-only integrity/schema/correlation checks.

The output binds the manifest, protected policy, Cashu inventory and identical
core image digest. Its `cashu_wallet_verified`, `combined_restore_authorized` and
`resume_authorized` are all false. The inventory digest is a link for the future
combined verifier; this command does not independently verify that inventory,
Cashu proof ownership, external providers or restoration readiness. A complete
combined workflow must verify the wallet inventory and bind the same core bytes.

The explicit ignored native fixture and hash-pinned root/network-namespace runner
cover a real native Cashu receipt in all four current-schema journals, unchanged
images, malformed scope/hash/image sets, ordinary restore refusal and a rehashed
wrong membership epoch. The [explicit root fixture](../testing/cashu-correlation-root-result-20261116.json)
passes, together with the
[full corrected-source gate](../testing/cashu-correlation-corrected-gates-20261116.json). Initialized combined native CLI evidence is recorded below.

### Combined offline verifier (initialized native acceptance)

`python3 deploy/scripts/verify-cashu-combined-backup.py SNAPSHOT --native-cli
PATH --native-cli-sha256 HEX` requires root and a reviewed native statectl
executable selected by exact digest. It freezes that executable before use.
The envelope contains exactly `combined.json`, `maintenance-policy.toml`, a
`cashu/` directory in the existing Cashu inventory format and a `native/`
directory in the distinct four-journal correlation format above. All are private
and root-owned; the native policy/bundle also require protected ancestry.

`combined.json` contains version `1`, a canonical nonzero `checkpoint_id`, and
`cashu_inventory_sha256`, `native_manifest_sha256`,
`maintenance_policy_sha256`. Both inventories use that checkpoint. The native
manifest links the exact Cashu inventory and maintenance policy digests. Its core
image digest must equal the Cashu inventory's core component digest, and the
protected native epoch must equal the Cashu core registration. The verifier runs
the existing Cashu checks and invokes the native correlation review on the same
images; it checks the returned bindings and repeats metadata checks afterward.
Cashu signing-broker and payout signing-broker journals remain separate.

The combined result reports inventory consistency only. It cannot prove that
writers were quiescent when the snapshot was taken, or discover external effects
that postdate a restored snapshot. It does not authorize restore/resumption,
credit, spending, rewards or feeding. Five focused Python checks cover cross-set
splicing and frozen executable selection. The [actual CLI composition](../testing/cashu-empty-combined-native-backup-20261116.json)
passes for initialized empty state: 14 Cashu components, two mint namespaces and
all four native journals, with exact core-byte linkage and Cashu/payout signer
substitution refusal. Funded state, production writer-quiescent collection and
rollback/external reconciliation remain unfinished.

The initialized inventory/combined-verifier fixtures currently back up wallet
seed/receiving keys and signing journals, not the scoped NIP-46 client credentials
or historical receiving-key windows. They cannot qualify full key recovery or
signer resumption. Those components and their public-client bindings still need
an explicit protected inventory contract and negative recovery tests. Do not treat
an empty initialized broker database as evidence that its credentials can be
reconstructed, or import the project signing key into the application to bypass
missing client recovery material.

The expanded native rehearsal copies seven selected executables. On this host,
that exceeds the small `/run` tmpfs. `test-cashu-private-run.py` requires fresh
mount and network namespaces, makes propagation private, and binds a temporary
root-owned `/var/tmp` directory at `/run` only for the child fixture. It unmounts
and removes that storage afterward. CI uses the same wrapper. This changes no
host mount, service, key, payment or physical feeder state.
