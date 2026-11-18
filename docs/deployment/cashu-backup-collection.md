# Cashu backup collection — copy normalization

The coordinated encrypted collector is implemented in source and under
qualification. It is not installed or accepted for production recovery.
`lightning-goats-cashu-statectl normalize-copy` is its first native primitive.
The initial [local native checkpoint](../testing/cashu-copy-normalization-local-20261117.json)
passes two positive and 21 refusal/interruption cases. The later
[special-file correction](../testing/cashu-copy-special-files-20261117.json)
passes two positive and 24 refusal/interruption cases, including marker/main/WAL
FIFOs with three-second fixture deadlines. All ten selected-source workflows
now pass at `7f8a2d7` ([CI evidence](../testing/cashu-copy-special-files-ci-20261117.json)).
The website dependency-install failure was an unreachable Ubuntu mirror; its
retry passed. The later coordinated collector is separate source.
It opens only explicitly supplied private copies;
it neither stops services nor loads wallet keys. Do not use it on installed state.
The [recovery inventory](cashu-recovery-inventory.md) remains authoritative for
the complete checkpoint, historical policies and recovery fences.

Build from the locked, separate wallet dependency graph:

```sh
cargo build --locked --manifest-path tools/cashu-compat/Cargo.toml \
  --bin lightning-goats-cashu-statectl
```

The root collector must first prevent admission, verify all affected writers
have stopped, retain their writer authorities, and copy the complete database
and committed WAL sets into a fresh root-owned staging directory. These steps
are requirements for the future collector, not effects of this helper. Preserve
uncertain source sidecars and all original state. A main-file-only copy of a
database with committed WAL data is incomplete.

The stage must be a canonical absolute directory, mode 0700 or stricter, beneath
root-owned ancestors with no group/other write permission. Every input and marker
must be a root-owned, private regular file with one link; aliases and symlinks
refuse. Descriptor opens use `O_NONBLOCK` so FIFO inputs refuse promptly before
SQLite access. The stage must contain `INCOMPLETE`, mode 0600 or stricter, with exactly:

```text
LIGHTNING_GOATS_CASHU_BACKUP_COPY_V1
```

Retain the trailing newline. Raw basenames begin `raw-`; output basenames begin
`normalized-`. Both end in `.db` or `.sqlite` and contain only ASCII letters,
digits, dots, underscores or hyphens. The invocation is:

```text
lightning-goats-cashu-statectl normalize-copy PRIVATE_STAGE RAW_NAME NORMALIZED_NAME MAX_BYTES
```

`MAX_BYTES` is a numerical limit from 4096 through 17179869184 bytes. Main/WAL
inputs, logical database size and normalized output are bounded. A 256 MiB SQLite
heap limit and a 60-second SQLite/copy deadline also apply. The caller must impose
its own total process deadline and aggregate checkpoint/storage budget.

The pinned SQLite 3.54.1 backup API includes committed WAL transactions while
preserving implicit row IDs and signed byte values. The helper checks integrity
and foreign keys, requires a standalone output with no WAL, hashes source main
and WAL before/after, verifies output identity, and syncs the file and stage.
Input shared memory may be rebuilt only inside this copied stage. SQLite's Unix
VFS refreshes the copied WAL's ownership even when unchanged; its ctime may
change. WAL inode, ownership, permissions, link count, size, modification time
and complete bytes must still match. Main-file and marker ctime changes refuse.
Rollback
journals, orphan shared memory and any preexisting output/sidecar refuse.

Success emits a coarse JSON receipt with input/output hashes and size. Its
`quiescence_verified`, `coordinated_backup_verified`, `restore_authorized` and
`application_runtime_started` fields remain false. A checksum certifies copied
bytes, not agreement with current mint, Strike or physical outcomes.

Failure emits no private paths or database errors. Keep `INCOMPLETE`, raw copies
and any partial normalized output. Reusing an output refuses; do not delete it
to conceal a failed attempt. This primitive does not finalize a backup, erase
evidence, replace an installed database, clear recovery markers or restart work.

The root/network-isolated fixture
`deploy/scripts/test-cashu-copy-normalization.py` exercises main-only and committed
WAL copies, implicit row IDs and signed bytes, path/ownership/alias/size refusals,
foreign-key/corruption refusal and interrupted-output retention. CI runs the
actual selected native CLI. Synthetic fixtures do not establish production writer
quiescence, coordinated encrypted collection or funded restore.

The collector must retain all native journals, the wallet journal and every
CDK namespace, both signer histories, historical keys/authentication, policies
and exact artifacts; verify their cross-store bindings; and encrypt the private
checkpoint. Wallet-only/core-only rollback and coordinated restore after external
effects still require separate qualification before any recovery fence can clear.

## Coordinated collection

`collect-cashu-backup.py` is an authenticated-root operation. It requires all six
services already masked/inactive with zero main/control PIDs, empty cgroups and
no processes under their runtime UIDs. It checks these conditions repeatedly,
holds the existing host maintenance lock and both Cashu writer locks, and binds
original file identities/hashes and directory metadata across collection and
encryption. It never stops, starts, masks or unmasks a service.

The policy explicitly attests administrative exclusivity. This complements the
observable service/UID/lock checks; it does not exclude another root administrator.
All six runtime directories and each retained mint namespace are inventoried.
Unknown files, aliases, special entries, changed originals, missing namespaces,
rollback journals and incomplete native/key verification refuse. Existing recovery
markers are retained; the collector never clears them.

The [policy example](../../deploy/cashu/backup-collection.json.example) is
deliberately invalid: exclusivity is false, UIDs are zero, paths/hashes need
replacement and no encryption recipient is selected. A protected operator policy
must name the actual six worker identities, all seven configurations, exact native
and encryption artifacts, explicit additional files and portable signer-client
key sources. Component, aggregate plaintext-plus-ciphertext and duration bounds
are mandatory. These storage limits are unrelated to financial execution limits.

Client key files contain 64 hexadecimal characters and optional LF. They must be
private root-owned recovery sources. A collection policy can also read the two
host-encrypted signer clients directly. For the shipped systemd bindings, add
these fields to the protected policy:

```json
{
  "systemd_signer_sources": true,
  "cashu_client_source": "/etc/credstore.encrypted/lg-cashu-nostr-client",
  "payout_client_source": "/etc/credstore.encrypted/cyberherd-signer-client"
}
```

Both files must share a protected root-private store. The collector requires
the current systemd host key and checks each envelope against its expected unit
credential name. It writes the portable bytes only to the private checkpoint
before age encryption. The collection metadata records the original ciphertext
hashes. Omit `systemd_signer_sources` to use the two portable files described in
the [policy example](../../deploy/cashu/backup-collection.json.example).
See the fenced installer below for reprovisioning on a replacement host.
External bunker permissions need separate acceptance.
`verify-recovery-keys PRIVATE_STAGE` uses
the pinned curve library to compare both client keys with their configured public
identities, reject shared/project identities, compare the receiving key, and
recompute the wallet seed's immutable journal binding. Explicit historical keys
are checked too; this does not establish historical descriptor-window completeness.

Extra files must explicitly include other required application credentials,
helper state, protected policies and supporting artifacts. Unknown files in the
inventoried runtime/helper directories refuse. Complete installed credential
coverage and historical-key administration remain acceptance requirements.

The root-owned capsule contains exactly `launch-cashu-backup.py`,
`collect-cashu-backup.py`, `stage-cashu-restore.py`, `review-cashu-restore.py`,
`reconcile-cashu-restore.py`, `verify-cashu-backup.py`,
`verify-cashu-combined-backup.py`, `apply-cashu-mint-policy.py`,
`inactive_file_transaction.py`, `install-cashu-restore.py` and `TOOL.json`.
The manifest has version 1, the selected 40-character source commit and SHA256
for each script.
Authenticate the bootstrap file externally before interpreting it, then use the
reviewed manifest hash with isolated Python:

```text
/usr/bin/python3 -I -S -B ROOT_CAPSULE/launch-cashu-backup.py TOOL_SHA256 POLICY POLICY_SHA256 NEW_PRIVATE_DESTINATION
```

This is an invocation contract, not an installation or activation instruction.
The bootstrap validates root ownership, protected ancestry, exact file inventory,
aliases/ACLs/capabilities and bounded, frozen source bytes before local imports.
The policy itself must be root-private and match its separately supplied digest.

The destination retains a private `plaintext` tree with raw main/WAL copies,
standalone normalized images, the linked native/Cashu reviews, verified recovery
keys, configurations and the exact selected programs. The verified
`receiving-key.bin` remains the wallet registration anchor. Any additional
active or retained key named by the wallet configuration is copied from its
canonical runtime filename and verified against its public binding. Use the
collector policy's explicit historical list for separately retained material
outside the configured inventory. The same normalized core
bytes enter both inventories by copying, never hardlinking. Only these disposable
copies are opened through SQLite or the key verifier.

Original SHM bytes are retained under a separate `.original` name. SQLite may
rebuild its working cache; that cache must not replace the retained sidecar or
invalidate the source-copy hash. The committed-WAL native fixture checks every
recorded copy hash as well as the decrypted archive, after first reproducing this
metadata mismatch. Source UID/GID/mode are retained for later reviewed recovery.

The verified tree is streamed through the pinned `tar` and age 1.3.2 programs
into `checkpoint.tar.age` using a plain `age1...` recipient. Ciphertext has a
remaining-storage hard limit, and both pipeline processes have bounded lifetime.
No private recovery identity is loaded by collection. On success, `COMPLETE.json`
binds the ciphertext hash, policy and checkpoint; only this collector's own
`INCOMPLETE` marker is removed. Failure retains raw copies, partial output and
the incomplete marker. Existing destinations always refuse.

Plaintext is retained for explicit operator handling. Collection does not certify
possession of the age recovery key, off-host retention, external provider/physical
outcomes, restore or resumption. Do not treat `COMPLETE.json` as permission to
start restored workers. Wallet/core/coordinated funded rollback remains required.

`prepare-cashu-backup-age.py` stages the exact official Linux amd64 release for
fixtures, checking its published archive and executable hashes and retaining its
LICENSE. It installs nothing. The expanded native fixture uses actual application
schemas, the native copy/key helper, native correlation verification and an actual
age decrypt/byte comparison. The disposable Actions rehearsal checks actual
runtime systemd masks, `/proc` runtime UIDs and the maintenance lock. The local
fixture substitutes service statuses and uses a private lock path. Both use
initialized empty journals.

## Stage a checkpoint for reconciliation

Keep the encrypted archive, collection receipt and independently recorded
ciphertext SHA256 together. On an offline recovery host, place copies of the
archive, receipt and native age identity in protected root-owned directories.
Use the same authenticated launcher and separately obtained local verifier
executables:

```sh
/usr/bin/python3 -I -S -B ROOT_CAPSULE/launch-cashu-backup.py TOOL_SHA256 stage \
  ARCHIVE ARCHIVE_SHA256 RECEIPT AGE_IDENTITY NEW_PRIVATE_DESTINATION \
  --age AGE_EXECUTABLE --age-sha256 AGE_SHA256 \
  --statectl CYBERHERD_STATECTL --key-verifier CASHU_STATECTL \
  --max-bytes STAGING_BYTE_BUDGET --deadline-seconds 300
```

Select a new child of a private root-owned directory. The storage budget
covers the temporary decrypted tar, extracted evidence and new fenced copies
at the same time. The root-private age identity contains one native X25519
key. The separately supplied age hash pins the executable before decryption.
The native verifier files must match the hashes bound to the collected policy.
Archived executables stay in the evidence tree with execution permissions off.

The launcher checks the ciphertext hash and receipt before decryption. The
stager accepts regular files and directories from the collector's USTAR format,
rechecks recorded copy hashes, reruns the combined inventory verifier, and
binds the recovered keys to the wallet journal and collected configurations.
It creates the recovery markers for all six roles before copying the normalized
images into `fenced-images/`. Configured wallet keys use their original filenames
in the fenced image. The remaining policies, signer credentials, separately
inventoried historical keys, raw sidecars and program bytes stay under
`checkpoint-evidence/`.

A successful stage contains `STAGED.json` and root-private images for review.
The receipt binds the collection record and `FENCED_IMAGES.json`, which lists
each staged file with its size and hash.
The temporary tar is removed after verification. A failed stage keeps its
`INCOMPLETE` marker and any private evidence already written. No installed
directory, runtime ownership, service policy or recovery marker is changed.
Reconcile each original mint, Strike and feeder outcome before considering
installation or resumption. `STAGED.json` records pending external reconciliation
and leaves resumption unauthorized.

The encrypted archive rehearsal copies the six fenced images into disposable
role directories with separate runtime UIDs. Each native command must reach
its recovery guard and leave the copied files unchanged. The Cashu wallet stops
before reading its seed or receiving key. The Cashu signer stops before loading
its NIP-46 client credential. Both report the recovery fence explicitly.

Use the same launcher to produce a private list of original IDs for provider
and feeder reconciliation:

```sh
/usr/bin/python3 -I -S -B ROOT_CAPSULE/launch-cashu-backup.py TOOL_SHA256 review \
  STAGED_DIRECTORY STAGED_RECEIPT_SHA256 --output NEW_PRIVATE_REPORT
```

Supply the SHA256 of `STAGED.json` from the reviewed stage. The command
rechecks every fenced file and reports pending Cashu acquisitions, original
mint melt quotes and split operations, Strike receive requests, financial
feed plans and feeder UUIDs. A pending financial feed plan appears even
before it creates a treasury intent or a feeder request. Its record binds
the ordered source allocations by SHA256 and shows any retained dispatch UUID.
The split list retains the original split and CDK operation IDs and the
submitted request digest when available. When independently
retained newer evidence exists, add `--reference REFERENCE_DIRECTORY
--reference-sha256 REFERENCE_RECEIPT_SHA256`. The report then includes
differing original records and rejects conflicting immutable identities or a
different wallet registration. The reference must preserve the exact staged core
event history before adding later events. This finds operations missing from a
stale image when the reference retained them. An empty worklist still requires
external verification for effects after the last captured checkpoint.

Keep the report private. Each report records the selected stage receipt hashes
and page parameters. If a category has a `next_after` UUID, run the same review
with `--group CATEGORY --after UUID` to retrieve its next page. The command reads
both stages without opening a live ledger and leaves all recovery fences in place.

### Verify one original Strike settlement

Select the exact intent from the fenced checkpoint that retains the original
invoice. The selector binds the checkpoint receipt hash, invoice, original
request UUID and intent digest in a private file. It uses the retained Strike
receive UUID if present. If settlement happened after the checkpoint, supply
the original provider receive UUID from separately retained evidence using
`--receive-id`. A conflict with a retained receive UUID refuses selection.

```sh
/usr/bin/python3 -I -S -B ROOT_CAPSULE/launch-cashu-backup.py TOOL_SHA256 review \
  STAGED_DIRECTORY STAGED_RECEIPT_SHA256 --strike-intent ORIGINAL_INTENT_UUID \
  --output NEW_PRIVATE_SELECTION
```

Review the selection and record its SHA256. Use the selected native executable
and the original scoped receive credential in a root-private file. Both the
credential fingerprint and the selected Strike origin must match the intent.
Use `--strike-origin` when reviewing an original request issued against an
explicit nondefault provider origin.

```sh
lightning-goatsctl verify-restored-cashu-strike \
  --selection PRIVATE_SELECTION --selection-sha256 SELECTION_SHA256 \
  --credential PRIVATE_ORIGINAL_RECEIVE_CREDENTIAL --output NEW_PRIVATE_OBSERVATION
```

The native command verifies the signed invoice before contacting Strike. It
reads the original receive request and completed receive. The BTC target,
request and receive UUIDs, invoice, payment hash and amount must agree. It
writes the single observation with its source hashes to a new private file.
The command opens no database and creates no invoices, payments or feeder
requests. Continued recovery requires independent mint and feeder evidence,
and the observation leaves the recovery fences in place.

### Observe the original mint melt and feeder request

Use the stage that retains the submitted mint operation. The selector checks
the original issued invoice, quote, mint, wallet epoch and CDK operation. It
copies the quote terms without inputs, blinded outputs or the wallet seed.

```sh
/usr/bin/python3 -I -S -B ROOT_CAPSULE/launch-cashu-backup.py TOOL_SHA256 review \
  STAGED_DIRECTORY STAGED_RECEIPT_SHA256 --mint-intent ORIGINAL_INTENT_UUID \
  --output NEW_PRIVATE_MINT_SELECTION
lightning-goats-cashu-statectl observe-restored-mint \
  PRIVATE_MINT_SELECTION SELECTION_SHA256 APPROVED_MINT_URL NEW_PRIVATE_MINT_OBSERVATION
```

The native tool verifies the signed invoice and performs one GET through the
mint-scoped HTTPS transport. The approved mint URL must match the original.
The reply must retain the quote identity, amount, fee reserve and expiry.
A paid observation requires the original payment preimage. The private
observation may contain that preimage and any returned blinded signatures.
Retain it with the checkpoint evidence. A pending or unpaid reply still leaves
the original reservation unresolved. Verify the independent Strike receive and
change ownership before credit or proof reconciliation.

```sh
/usr/bin/python3 -I -S -B ROOT_CAPSULE/launch-cashu-backup.py TOOL_SHA256 review \
  STAGED_DIRECTORY STAGED_RECEIPT_SHA256 --feeder-request ORIGINAL_REQUEST_UUID \
  --output NEW_PRIVATE_FEEDER_SELECTION
lightning-goatsctl observe-restored-cashu-feeder \
  --selection PRIVATE_FEEDER_SELECTION --selection-sha256 SELECTION_SHA256 \
  --gateway-origin REVIEWED_GATEWAY_ORIGIN --output NEW_PRIVATE_FEEDER_OBSERVATION
```

Use the independently reviewed gateway origin for the selected installation.
The command performs one status GET for the original request UUID. The gateway
may retain an acknowledgment while answering. It receives no actuation command.
The private report retains the gateway result, stage hashes and any contradiction
with the checkpoint's finalized outcome. A missing or incompatible reply yields
no observation. A pending or ambiguous result remains unresolved. Keep all
fences until the wider recovery review resolves external effects and stale
ledger history.

### Compare observations with the checkpoint

Keep the unmodified native observation files in a root-private directory.
Record each observation's reviewed SHA256 in a private manifest beside those
files. Supply one original treasury pair, one original feeder request, or both.
Use `null` for an absent selection. A manifest for both uses this shape:

```json
{
  "version": 1,
  "treasury": {
    "intent_id": "ORIGINAL_INTENT_UUID",
    "mint": {"file": "mint.json", "sha256": "MINT_OBSERVATION_SHA256"},
    "strike": {"file": "strike.json", "sha256": "STRIKE_OBSERVATION_SHA256"}
  },
  "feeder": {
    "request_id": "ORIGINAL_REQUEST_UUID",
    "gateway_origin": "REVIEWED_GATEWAY_ORIGIN",
    "observation": {"file": "feeder.json", "sha256": "FEEDER_OBSERVATION_SHA256"}
  }
}
```

```sh
/usr/bin/python3 -I -S -B ROOT_CAPSULE/launch-cashu-backup.py TOOL_SHA256 reconcile \
  STAGED_DIRECTORY STAGED_RECEIPT_SHA256 PRIVATE_EVIDENCE_MANIFEST MANIFEST_SHA256 \
  --reference REFERENCE_DIRECTORY --reference-sha256 REFERENCE_RECEIPT_SHA256 \
  --output NEW_PRIVATE_COMPARISON
```

Omit the reference options when every selected original exists in the staged
checkpoint. A reference must preserve the earlier event prefix and wallet
registration. Each provider observation must trace to one selected checkpoint.
The comparison reconstructs the exact original selector bytes, matches the
mint and Strike observations to one invoice and verifies the paid preimage and
BTC amount. It compares retained completion and wallet accounting totals. A
later completion or feeder confirmation can be present in the reference and
missing from the older stage. The report also records gateway conflicts and
counts other differences in the worklist.

The report retains observation hashes and accounting results. Keep the raw
observations alongside it for separate ownership and external-history review.
This comparison does not discover all external operations, revalidate current
mint change ownership, apply accounting changes or authorize clearing a fence.

For a batch, use manifest `version: 2` and arrays for `treasury` and `feeder`.
Each entry uses the corresponding version 1 fields. Use an empty array only
when the corresponding worklist has no entries. The command independently
derives the required treasury IDs from the union of `strike_receives` and
`mint_melts` and the feeder IDs from `feeder_requests`. It rejects missing,
duplicate, and unrelated originals before comparing the observations. A batch
can include `custody_all` for every retained mint in one of the selected stages.

A batch supports up to 1,000 originals of each type in a 512 KiB manifest.
If a worklist is larger, use the paged worklist and individual observations.
Some pending operations have no provider identifiers yet. A complete batch
requires matching evidence for every selected original, so preserve any such
operation separately behind its fence. The report still counts unresolved
splits and acquisitions. Recovery decisions also require evidence for those
operations and any effects after the selected reference.

### Observe the mint's current custody state

Select a configured mint ID from the fenced stage. The selector checks the
verified namespace image hash and the original sat-denominated mint URL. It
reads up to 128 proof points in storage order and includes their local states
and amounts. The selection keeps spending secrets and signatures in the wallet
image. Use the configured and independently reviewed original mint URL.

```sh
/usr/bin/python3 -I -S -B ROOT_CAPSULE/launch-cashu-backup.py TOOL_SHA256 review \
  STAGED_DIRECTORY STAGED_RECEIPT_SHA256 --custody-mint MINT_ID \
  --output NEW_PRIVATE_CUSTODY_SELECTION
lightning-goats-cashu-statectl observe-restored-custody \
  PRIVATE_CUSTODY_SELECTION SELECTION_SHA256 APPROVED_MINT_URL NEW_PRIVATE_CUSTODY_OBSERVATION
```

The observer submits only public proof points to the mint's proof-state
lookup. Its response must cover the exact selected points once each. A local
unspent proof reported spent by the mint is stale custody. Pending and other
nonterminal observations remain unresolved. An empty selection needs no
lookup. For more than one page, use the recorded `next_after_y` with
`--custody-after` and a new selection and observation file until it is `null`.
Keep all pages private and together with their selected checkpoint.

The `reconcile` manifest accepts a `custody` entry for one mint. Set
`staged_receipt_sha256` to the stage used for every selection in the batch. Use
the primary stage hash or its independently reviewed reference hash. List all
native observation files in the order their selections were produced. Set the
`treasury` and `feeder` entries to `null` for a custody-only comparison.

```json
{
  "version": 1,
  "treasury": null,
  "feeder": null,
  "custody": {
    "mint_id": "MINT_ID",
    "staged_receipt_sha256": "STAGED_RECEIPT_SHA256",
    "observations": [{"file": "custody-page-1.json", "sha256": "OBSERVATION_SHA256"}]
  }
}
```

The comparison reconstructs every page from the verified namespace image,
checks the reported amounts and states, and requires an ordered sequence that
covers the complete selection. It records the observation time range because
the mint may change state between pages.

Mint-reported unspent value is an observation of those points. Continue to
verify ownership, old and newly discovered operations, database history, and
external accounting before any restore or resumption decision.

### Compare custody across the entire wallet checkpoint

Inventory the selected checkpoint before gathering custody evidence. This
includes every retained mint namespace, including empty namespaces and mints
with receiving disabled. The shared legacy CDK store must have no retained
proofs or wallet operations.

```sh
/usr/bin/python3 -I -S -B ROOT_CAPSULE/launch-cashu-backup.py TOOL_SHA256 review \
  STAGED_DIRECTORY STAGED_RECEIPT_SHA256 --custody-inventory \
  --output NEW_PRIVATE_CUSTODY_INVENTORY
```

Generate selections and native observation files for each namespace in that
inventory using the preceding commands. Include the observation for an empty
namespace. Its native observer succeeds without a mint request. Supply the
complete ordered set of observation files per mint to `reconcile` using the
`custody_all` field in place of `custody`.

```json
{
  "version": 1,
  "treasury": null,
  "feeder": null,
  "custody_all": {
    "staged_receipt_sha256": "STAGED_RECEIPT_SHA256",
    "mints": {
      "first-mint": [{"file": "first-page-1.json", "sha256": "FIRST_OBSERVATION_SHA256"}],
      "second-mint": [{"file": "second-page-1.json", "sha256": "SECOND_OBSERVATION_SHA256"}]
    }
  }
}
```

The comparison independently derives the complete mint set from the selected
wallet journal and verified images. It refuses omitted namespaces, unknown
stores, incomplete initialization, and missing or mismatched pages. The
report retains per-mint results, aggregate sat totals and the time range of
the observations. `mint_unspent_held_sats` counts the part of the mint-reported
unspent value that the local wallet still holds outside its spendable balance.
These are separate reads made during that range. The report preserves every
fence and keeps resumption unauthorized.

Use evidence manifest version 3 to inspect both checkpoints together. Keep
the version 2 treasury and feeder arrays. Set `custody_all` to the complete
batch from the primary stage and `reference_custody_all` to the complete batch
from the separate `--reference` stage. Both fields use the shape above, with
their respective receipt hashes and independently generated selections.
Each batch must cover every namespace in its own checkpoint. The report also
counts operations still pending in the reference, even when the reference
and primary share the same pending record. It counts original operations
missing from the reference regardless of the worklist page size.

The funded two-mint rehearsal compares native observations from the original
synthetic issuers after their melts. The pre-melt checkpoints still list
250 sats and 150 sats as unspent. The comparison identifies all 400 sats as
spent and preserves the original checkpoint bytes. The same checks run after
a lost melt reply. The encrypted funded installation also observes the later
checkpoint and compares the two verified custody batches together.

## Install the fenced images on a recovery host

Use the exact recovered configuration, runtime UIDs and GIDs, and independently
installed executables recorded by the selected collection policy. Provision them
on the recovery host and keep the six units masked and inactive. The original
writers must be stopped and the operator must have exclusive maintenance
control. The install command checks the selected local units and runtime UIDs.
An older staging receipt without a collection digest requires a new stage from
the original authenticated ciphertext.

```sh
/usr/bin/python3 -I -S -B ROOT_CAPSULE/launch-cashu-backup.py TOOL_SHA256 install-fenced \
  STAGED_DIRECTORY STAGED_RECEIPT_SHA256 ORIGINAL_POLICY ORIGINAL_POLICY_SHA256
```

Use the independently recorded receipt and policy digests. The command checks
the original config paths, bytes and metadata, runtime executable hashes and
permissions, and every staged image. It prepares all missing images privately
under their respective state-directory parents. It publishes a complete fenced
image per directory, with mode `0700` on the role directory and mode `0600` on
its files. An empty directory with the exact runtime ownership and mode may be
replaced. A nonempty destination must be an exact fenced image of the selected
checkpoint. Any other destination is preserved and the command refuses.

If publication is interrupted after some roles, keep all workers masked and run
the same command with the original digests. Exact previously installed roles
are verified and reused. All published roles carry their fences even before
the command has verified the entire installation. The result reports the
checkpoint and confirms all six entries. External settlement, custody checks,
and authorization for resumption still need separate evidence.

### Switch an existing fenced installation to a later reconciled checkpoint

Keep every unit masked and keep both stages and the version 3 evidence
manifest. Select the later stage as the install target, then give the
installer the exact older stage that is already installed.

```sh
/usr/bin/python3 -I -S -B ROOT_CAPSULE/launch-cashu-backup.py TOOL_SHA256 install-fenced \
  LATER_STAGE LATER_RECEIPT_SHA256 POLICY POLICY_SHA256 \
  --advance-from INSTALLED_OLDER_STAGE OLDER_RECEIPT_SHA256 \
  --advance-evidence PAIRED_EVIDENCE PAIRED_EVIDENCE_SHA256
```

The installer recomputes reconciliation from the pinned manifest and both
stages. It checks the later stage retains all earlier operations, has no
pending operations in the worklist, including financial feed plans that
still reserve credit, and agrees with the original treasury
and terminal feeder outcomes. A completed treasury operation must retain
its accounting and wallet cleanup record. The later custody observations
must show no stale, unresolved, or locally held mint-unspent value. A
verified spendable balance is allowed.

For each runtime directory, the installer parks the exact older fenced
image in a root-private directory on the same filesystem, then publishes
the later fenced image. It reports the retained paths when all six pass
verification. An interruption can leave a mix of old and new fenced
directories or an empty slot whose older image is already retained.
Keep the units masked and retry with the same stage, policy, and manifest
digests. A different operation or unexpected installed image is refused.
Service activation and resumption authorization need separate verification.

### Prepare the installed checkpoint for paused recovery

After a verified advancement, keep all six units masked. Have the operator
review a root-private approval that binds the same stages, policy, and paired
evidence. Record the approval file's SHA256 separately.

```json
{
  "version": 1,
  "purpose": "cashu-paused-recovery",
  "previous_stage_sha256": "OLDER_RECEIPT_SHA256",
  "stage_sha256": "LATER_RECEIPT_SHA256",
  "policy_sha256": "POLICY_SHA256",
  "evidence_sha256": "PAIRED_EVIDENCE_SHA256"
}
```

```sh
/usr/bin/python3 -I -S -B ROOT_CAPSULE/launch-cashu-backup.py TOOL_SHA256 install-fenced \
  LATER_STAGE LATER_RECEIPT_SHA256 POLICY POLICY_SHA256 \
  --advance-from OLDER_STAGE OLDER_RECEIPT_SHA256 \
  --advance-evidence PAIRED_EVIDENCE PAIRED_EVIDENCE_SHA256 \
  --paused-gate prepare --paused-approval APPROVAL APPROVAL_SHA256
```

The capsule rechecks reconciliation, all six installed images, and the
retained older copies. It atomically replaces the core, wallet, and Cashu
signer markers with root-owned symbolic links at the same paths. The paused
programs interpret the link text as authorization for that role. Ordinary
startup still rejects the marker. CyberHerd's three markers retain their
original form throughout. The capsule starts no services and reports
`resume_authorized: false`.

Use the same approval and replace `prepare` with `restore` to put the exact
hard markers back. Keep the units masked and the role processes stopped
through both commands. An interruption can leave a mix of prepared and
hard markers. Resume either direction with the original digests. The
command refuses an unexpected image or a marker bound to another approval.
Restore the hard markers before collecting another checkpoint.

### Reprovision the two archived signer clients

On a qualified recovery host, add `--signer-credential-store /etc/credstore.encrypted`
to the command above. Use the existing canonical root-private credential
directory and an initialized systemd host key that belongs to this machine.
For a fresh host, first use the
[credential-store initializer](../../deploy/scripts/initialize-staging-credential-store.py)
to check for orphaned encrypted files and initialize the key. Keep the six
runtime roles masked and inactive.

The installer verifies both archives against the collection receipt selected at
staging. It takes only the portable Cashu NIP-46 client and the independent
CyberHerd payout NIP-46 client. Their original unit credential names are
`cashu-nostr-client` and `nostr-client-key`. It encrypts those keys for the
current host as `lg-cashu-nostr-client` and `cyberherd-signer-client`,
respectively. Plaintext passes through the systemd tool over pipes. The installer
decrypts each candidate privately and checks the exact original bytes before
publishing it. It verifies the installed journals and their fences first.
Existing credentials are preserved. An exact retry accepts a credential only
when it decrypts on this host under the intended name to the archived key.
If interrupted, keep the state fenced and retry with the same stage and policy
digests. The result lists the two verified signer roles without disclosing keys.

Validate the installed unit bindings and bunker permissions before signing
acceptance. Strike, webhook, and other independently managed credentials have
their own recovery requirements.

The encrypted fixture rehearses installation using six disposable runtime
UIDs. It verifies a retry after interruption, rejects a changed configuration
and prior state, and confirms native startup refuses all six installed copies.
The Actions job invokes the authenticated command with real masks to resume the
interrupted attempt. It verifies that a process running under the wallet UID
blocks installation even with every unit masked.

The funded rehearsal starts with two acquisitions credited by the core. It
captures both mint stores before the first melt, after the original Strike
invoice is issued and bound. The same settlement fixture subsequently loses a
melt reply and completes both original conversions. The older core and wallet
snapshots then go through the coordinated collector, age encryption, the
authenticated stager, and fenced installation under real systemd masks. The
restored images must preserve the original pending invoice, both credited
receipts, 250 and 150 sats of old local custody, and every startup fence.
The older stage's worklist must still identify that original unsettled invoice.

The same rehearsal seals a later reference after both settlements, the
simulated feeder confirmation and one new 16-sat credit below the feeding
threshold. It selects proof points from both checkpoints and the original
settled quotes from the later reference. The original
synthetic mints answer those reads. The native Strike observer
checks both receives against the original loopback provider. The feeder
observer checks the original UUID against the fixture gateway. The authenticated
comparison must find 400 stale sats, 396 sats in the two later Strike deposits
and one feeder confirmation missing from the older checkpoint. It verifies
16 sats unspent in the later reference and zero stale or unresolved mint
value. It refuses omitted operations, missing later custody evidence and
evidence rebound across checkpoints. Both
stages and the installed files stay byte-identical with their fences intact.
The rehearsal then switches the installed directories to the verified later
checkpoint. It interrupts once after retaining an older directory and once
after publishing a later image. The same command completes the switch and
keeps the previous six images available in protected storage.

### Paused core, wallet, and signer readiness

The core daemon can serve read-only recovery status on the configured loopback
listener. Use the flag on a disposable, verified recovery copy under the
runtime identity.

```sh
lightning-goatsd --recovery-paused --config RECOVERY_CONFIG
```

The mode checks the existing ledger's exact migration history and honors the
installed recovery marker. It opens SQLite read-only with committed WAL
visibility. It never initializes or migrates a ledger. Only `GET /healthz`
and `GET /api/v1/status` are registered. Status reports `recovery-paused`,
available feed credit, and the unresolved feeder request ID if applicable.
The daemon skips provider credentials, payment and WebSocket routes, all
financial workers, receipt sockets, Nostr, and the feeder gateway. The flag
does not grant authority to clear a marker or resume financial processing.

The wallet has a matching mode under its dedicated runtime identity.

```sh
lightning-goats-cashud recovery-paused WALLET_CONFIG
```

It honors the recovery marker and holds the existing wallet writer lock.
It verifies the wallet identity, epoch, seed and receiving keys, plus the
retained mint-to-store bindings. It reads journal custody and deposit totals
without running database migrations or starting listeners and workers. The
mode prints one JSON readiness record and holds the lock until SIGINT or
SIGTERM. The reported totals come from the recovered journal.

The Cashu signer verifies offline recovery readiness under its dedicated
runtime identity with the original systemd credential binding.

```sh
lightning-goats-cashu-signerd recovery-paused CASHU_SIGNER_CONFIG
```

It honors the marker, takes its existing writer lock, and opens only the
existing signing journal read-only. It checks the journal registration, the
dedicated NIP-46 client identity, and the pinned helper files. It reports the
retained request count and releases the lock on SIGINT or SIGTERM. The signer
opens no socket and performs no signing or publication.

Newly staged wallet and Cashu signer images include private empty lock files
bound into the staging manifest. The stager creates these files for the
replacement installation. Stage the original archive again with the current
capsule when an earlier staged wallet lacks its lock file.

The funded fixture interrupts both marker transitions, verifies exact retry
and reversal, and refuses a different approval. It exercises ordinary
startup refusal with the paused markers present. It starts the three binaries
pinned by the backup policy directly against the installed disposable stage
with its original configurations and role UIDs. The hosted test uses temporary
systemd units while the six ordinary units stay masked. It verifies read-only
mounts for each started service's state and configuration, and loads the
archived synthetic signer credential through systemd. Both start and stop
cycles preserve the bytes, ownership, permissions, and file identities of
all six installed role directories.

The fixture separately clones the installed core, wallet, and Cashu signer
images with their authorized markers to exercise rejection cases and enabled
settings. It starts the three binaries twice under separate UIDs with no
external network access. The core
uses paused recovery with a canary configuration that enables Cashu intake
and configures receipt import. The test starts the core without credentials,
probes for rejected effectful HTTP routes, checks that the receipt socket
stays absent, and confirms neither loopback provider receives a connection.
The wallet uses its paused mode with acquisition, receipt delivery and
treasury enabled in the test configuration. The test verifies writer
exclusion, checks that intake and treasury sockets stay absent, and tests
refusal of a missing lock, missing mint store, and mismatched seed. The signer
retains an original request with one failed attempt from the disposable
helper. Its paused starts hold the writer lock without opening a socket or
replaying that request. The hosted test supplies the exact archived synthetic
client credential through systemd after reprovisioning and verifies both
clean shutdowns. The three CyberHerd workers refuse their disabled settings.
Each cycle must preserve 16 sats of available feed credit, 16 sats of wallet
custody, completed Strike deposits totaling 396 sats, and the original
simulated feeding. The test compares durable wallet and signer bytes, checks
the core, wallet, and signer fences before starting copies, and confirms all
six installed role images match the original fenced reference after reversal.

Ordinary production startup, removal of the recovery fence, and live provider
acceptance still require separate qualification.
