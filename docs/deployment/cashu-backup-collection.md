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
private root-owned recovery sources. A systemd host-bound encrypted credential
blob is **not** such a portable source. Export/reprovisioning and external bunker
permissions need separate acceptance. `verify-recovery-keys PRIVATE_STAGE` uses
the pinned curve library to compare both client keys with their configured public
identities, reject shared/project identities, compare the receiving key, and
recompute the wallet seed's immutable journal binding. Explicit historical keys
are checked too; this does not establish historical descriptor-window completeness.

Extra files must explicitly include other required application credentials,
helper state, protected policies and supporting artifacts. Unknown files in the
inventoried runtime/helper directories refuse. Complete installed credential
coverage, portable export/reprovisioning and historical-key administration remain
acceptance requirements; a short manually declared extras list does not close them.

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
mint melt quotes and split operations, Strike receive requests and feeder UUIDs.
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
the observations. These are separate reads made during that range. The
report preserves every fence and keeps resumption unauthorized.

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
checkpoint and confirms all six entries. External settlement and custody checks,
credential provisioning, and authorization for resumption are separate steps.

The encrypted fixture rehearses installation using six disposable runtime
UIDs. It verifies a retry after interruption, rejects a changed configuration
and prior state, and confirms native startup refuses all six installed copies.
The Actions job invokes the authenticated command with real masks to resume the
interrupted attempt. It verifies that a process running under the wallet UID
blocks installation even with every unit masked. Funded recovery still requires
its own qualification.
