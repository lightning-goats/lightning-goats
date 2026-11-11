# CyberHerd journal retention and blocked restore staging

`cyberherd-statectl` is an offline local operator tool for the core paid ledger,
membership/interaction state, outgoing-payment journal and outgoing signer
journal. It has no provider, credential-loading, signing, feeder-request,
service-start, unmask or network-configuration command. It does not install or
activate CyberHerd. The legacy `restore-stores.py` refuses CyberHerd schemas.

## Retention and protected scope

The initial policy retains **all financial, replay and signed-delivery history**.
There is no automatic pruning, monetary-record TTL, deletion command or age-based
reuse of an invoice, source, quote, payment, request UUID or event ID. Membership
reset starts a new cycle without erasing replay evidence. Completed, failed,
conflicted and uncertain transfers retain their original correlations; cost
reviews and both journals' acknowledgments remain immutable. Exact signed Nostr
bytes and publication receipts remain part of recovery state.

An accepted private manifest records each image's bytes/SHA256, per-table counts,
pending/unknown feeds, unresolved provider submissions, execution claims and
payout-held sats. Its policy is `retain_all_financial_replay_and_signed_history`.
These are capacity/recovery reports, never wallet balances or execution authority.
Monitor storage and successive private reports; provision capacity before it is
exhausted. Never clear journals to make room. Retain separately controlled,
protected recovery copies under the operator's backup policy. This tool does not
upload, encrypt, rotate or delete backups. Checksums detect corruption; they do
not authenticate an untrusted backup. Preserve provenance and custody separately.
Keep manifests, paths, counts and signed events private, out of public issues.

Resolve `deploy/cyberherd/state-maintenance.toml.example` using the actual existing
four paths, separate non-admin UIDs/GIDs, unit names, project owner, core stream
epoch and Strike environment/account/credential fingerprint. The fingerprint is
an identity commitment, not a key. The example's root UIDs/nil UUIDs are invalid.
Do not invent an epoch or rebind a restored payout account. Check fields without
reading databases, credentials or services:

```sh
cyberherd-statectl --config /path/to/reviewed-state-maintenance.toml check
```

Operational commands require the kernel root identity, canonical root-owned
policy and protected root-owned ancestors. Source databases and existing WAL/SHM
must be private single-link regular files owned by their configured runtime
UID/GID inside that runtime's canonical private directory; higher ancestors must
be root-owned and protected. Output requires an existing protected root-owned
parent. The command exclusively creates its final directory, never overwrites.

Keep protected service policies, installed binaries/provenance, reviewed cost
approvals/evidence and credential recovery procedures separately. The tool backs
up **no API keys, webhook secrets, NIP-46 credentials or secret configuration**.
Its maintenance policy is included in the private manifest. Verification compares
the exact policy-file hash as well as parsed scope; changing its bytes requires
deliberate corresponding recovery-policy review.

## Consistent snapshot

Select an operator maintenance window and stop **all four writers together**.
Prevent socket/timer/helper/administrative writers and unrelated processes from
opening the journals throughout the window. Explicitly mask the four selected
system units. This includes the core receive service: schedule the interruption
and preserve issued requests/settlements. No command here stops it implicitly.
Do not test this tool against a running production stack.

`--writers-stopped` requires attestation and repeated read-only `systemctl show`
checks: exact ID, loaded/masked, inactive/dead, MainPID zero, persistent/runtime
mask and no populated remaining cgroup. Missing/active/unmasked units fail.
These checks cannot prove an administrator has not started another writer outside
those units; the no-other-writers maintenance attestation remains necessary.

After reviewed installation, select a fresh destination:

```sh
sudo /usr/local/bin/cyberherd-statectl \
  --config /etc/cyberherd/state-maintenance.toml \
  snapshot --destination /var/lib/cyberherd-backups/OPERATOR_CHOSEN_NEW_ID \
  --writers-stopped
```

The tool opens originals read-only and copies main databases plus committed WAL.
It never opens SQLite/checkpoints original sources, avoiding root-owned source
sidecars. SHM is observed for changes but is not backup truth or copied. Source
file fingerprints, directory identity and sidecar presence are rechecked before
acceptance. Changed state leaves `INCOMPLETE`; retain privately for diagnosis,
never treat that directory as an accepted backup.

Only copies are parsed with native SQLite 3.54.1, read-only/query-only and trusted
schema disabled. Checks cover integrity, foreign keys, exact migration checksums,
membership/core cursor and epoch, payout/provider identity, original reservation
and frozen plan, execution claims and original quote bytes, exact final/cost ACKs,
signer invoice intents and valid signed requests. A complete main-only copy is
opened immutable to prevent a generated empty WAL. Copies with an existing WAL
retain the regular read-only WAL reader and its permitted private SHM read-index;
making those images immutable would ignore committed WAL pages. Byte hashes
remain authoritative for both layouts. No migration is applied.
Pending delivery/ACK work is retained. After native inspection, image hashes and
source stability are checked again, the manifest/directory synced, and
`INCOMPLETE` removed. Success prints only operation/ID/acceptance and
`resume_authorized=false`; detail stays in root-private `MANIFEST.json`.

```sh
sudo /usr/local/bin/cyberherd-statectl \
  --config /etc/cyberherd/state-maintenance.toml \
  verify --bundle /var/lib/cyberherd-backups/OPERATOR_CHOSEN_NEW_ID
```

Verification rechecks all private images, schema, scope and report without live
source access. It does not authorize service resumption. Normal resumption after
**backup only** is a separate maintenance action; do not adopt copied journals.

## Review retained reward backlog

The native `review-rewards` command reads only an accepted four-journal backup
and creates a separate, new private report directory. It verifies the complete
backup first and again after reading it, binds the report to the exact manifest
hash/ID, and rechecks all image bytes before acceptance. Existing backup manifests
and schema remain unchanged. A copied committed WAL is read normally; a complete
main-only image uses immutable mode. No live store opener, credential, provider,
network, planning/reservation or service-control operation is available.

After installing the qualified native tool, choose a separate unused destination:

```sh
sudo /usr/local/bin/cyberherd-statectl \
  --config /etc/cyberherd/state-maintenance.toml \
  review-rewards --bundle /var/lib/cyberherd-backups/ACCEPTED_BACKUP_ID \
  --destination /var/lib/cyberherd-reviews/NEW_REVIEW_ID
```

The destination's parent must already be protected and root-owned. The report
cannot be created inside the accepted backup. `REVIEW.json` and its directory
are private; stdout contains only operation/review ID/acceptance and
`activation_authorized=false`. A partial write retains `INCOMPLETE`. This reads
the accepted copies and does not require stopping the current services. Creating
a **new current backup** still requires the normal explicit stopped-writer window.

The report examines every retained feeding barrier for the selected owner. It
separately counts unplanned zero-allocation, unplanned unfunded and unplanned
funded positive-allocation barriers, closed `no_payout` plans, plans awaiting
reservation (including a lost acknowledgment) and attached reservations. Saved
allocation/cycle/revision comes from each immutable barrier, not today's settings.
It checks the original completed intake, actual confirmed core debit and consumed
funding manifest, and any retained plan hash, budget/row sums and reservation
correlation. Contradictory or oversized records refuse the report; none is
silently skipped. At most 32 non-closed private UUID examples are retained,
with an explicit remaining-example count; classification itself is never truncated.

Pending intake/feeding inputs, the membership cursor and core event head,
unreceived payment/feeding event counts beyond that cursor,
unresolved physical feeds, unfinalized core reservations, unresolved provider
submissions and retained payout holds remain visible. Empty planned-work counts
alone cannot prove there is no earlier work left to process. A closed zero plan
is not reopened, and this command neither closes an unplanned plan nor reserves it.

This is evidence **as of the selected backup**, not a live balance, current
recipient qualification, complete external payment history or activation grant.
`external_reconciliation_required` stays true even for an empty backlog. A future
financial maintenance transaction must bind its review to the exact current
stopped-writer backup, preserve unresolved work and obtain the separately required
operator/provider authority. Do not edit an old report to make a later backup pass.

## Restore staging and external reconciliation

A matching backup can predate an external payment or physical feed. Rolling back
core and payout together can erase **both** sides' local evidence. Internal
consistency or zero pending work therefore cannot prove another payment/feed is
safe. Never automatically resume, rotate the epoch or create fresh IDs for
uncertain work. Preserve the independent household gateway/owner UUID history;
this VPS tool does not restore or reset that authority.

With all writers stopped/masked and no other writers, stage to another new parent:

```sh
sudo /usr/local/bin/cyberherd-statectl \
  --config /etc/cyberherd/state-maintenance.toml \
  stage-restore --bundle /var/lib/cyberherd-backups/OPERATOR_CHOSEN_NEW_ID \
  --destination /var/lib/cyberherd-recovery/OPERATOR_CHOSEN_NEW_ID \
  --writers-stopped
```

The top stays root-private. Children `core`, `membership`, `payout`, `signer`
receive byte-identical database/WAL images and original configured UIDs/GIDs.
Each child first receives a root-owned `CYBERHERD_RECOVERY_REQUIRED` marker.
All four normal store openers reject it before creating/opening/migrating state;
even a dangling marker symlink blocks. Epoch, UUIDs, reservations, cursors,
signatures, provider IDs and holds remain exact. The tool does not install staged
state, remove markers, edit configuration, start units or grant access through
the root-private parent.

Keep execution disabled in protected configuration and maintain network/service
fences. A separately reviewed recovery procedure must reconcile authoritative
Strike history and original quote/payment IDs **through the outage and backup
interval**, including submissions missing locally. Reconcile original feeder
UUIDs with the durable physical owner. Retain signed bytes/relay receipts for
exact retry, without replacement signatures. Recover cost approval/evidence
separately and preserve unresolved holds. Only a concrete operator-approved
reconciliation/adoption procedure can install resolved state, reconstruct absent
immutable evidence where needed, and eventually clear markers/unmask units.
No automatic clearance command is provided. Missing/disputed evidence leaves
affected work stopped. Retain originals and partial/staged sets.

### Provider evidence checked against current documentation

The 2026-11-11 API review confirms that
[`GET payments/{id}`](https://docs.strike.me/api/get-payment-by-id/)
requires `partner.payment.read` and returns one known payment's state, amounts
and fees. This supports reconciliation of retained original payment IDs; it
cannot by itself discover an execution whose entire local identity was lost
after the backup. Do not invent a payment-list endpoint, treat a guessed ID's
404 as evidence of no payment, or interpret a wallet balance as complete history.

Strike's documented
[event history](https://docs.strike.me/api/get-events/)
requires `partner.webhooks.manage`, supports created/event-type filtering and
pagination, and can report an unknown total count. The existing four-scope payout
key must not gain that administrative permission for routine execution. Separate
operator-controlled provider history/exports may assist recovery; pagination,
coverage and retention still need verification before claiming the complete
backup-to-quiescence interval. An empty page or absence of a locally known ID
does not authorize clearing recovery markers or releasing holds.

[`PATCH payment-quotes/{id}/execute`](https://docs.strike.me/api/execute-payment-quote/)
is an execution operation, not a read-only status probe. It must not be called
from snapshot verification or an operator's evidence-collection preview. The
runtime's separately fenced original-quote recovery retains its original
execution claim and financial authorization; this does not turn a restored
journal or a new quote into authority. The
[quote-creation contract](https://docs.strike.me/api/create-lightning-payment-quote/)
returns the original quote ID for duplicate idempotency keys, which is useful
only while the exact original intent/idempotency identity remains available.

These findings narrow the required evidence, not the full recovery obligation.
No provider API request, credential change, marker clearance or adoption was
performed for this documentation review. The concrete adoption implementation
and installed rehearsal remain unfinished; ordinary backup/blocked staging
commands above continue to grant no resumption authority.

## Additive private-contact comparison

The explicit `snapshot-before-contact --destination NEW_PRIVATE_BUNDLE --writers-stopped`
and `verify-contact-upgrade-source --bundle PRIVATE_SCHEMA_23_BUNDLE` operations
support only the exact compiled schema-23 upgrade source. Snapshot uses the same
four stopped/masked writer, distinct file-owner, bounded copy, committed WAL and
unchanged-source checks as ordinary snapshot. Verification requires exact native
schema checksums, four-journal scope and all private image hashes. They neither
migrate nor relax the ordinary current-schema snapshot/verify/restore paths.
The exception is deliberately pinned to code with exactly 24 core migrations;
it refuses a current source, unknown/dirty prefix or preexisting contact schema.
Its backup can be compared for this additive upgrade but cannot enter ordinary
restore staging. This also allows a reproducible installed upgrade rehearsal
without executing a historical build merely to obtain a schema-23 snapshot.

`compare-contact-upgrade --bundle ORIGINAL_SCHEMA_23_BACKUP --candidate-copy MIGRATED_FOUR_JOURNAL_COPY`
is a separate native comparison, not a migration or recovery operation. It requires
the protected maintenance policy and a complete private source backup whose exact
schema-23 checksums, images, scope and report remain valid. The candidate must
already be a separate schema-24 copy with all four journals. Every original table,
column, row ID, typed value, signed byte and migration-history prefix must match.
Only the compiled contact DDL and three empty private tables may be added; other
roles' schemas and data remain exact. Image hashes are checked again afterward.

The command never changes live journals, installs files, calls a provider, signs,
restores, removes recovery markers or authorizes service resumption. It does not
make a schema-23 backup eligible for ordinary current-schema restore. A qualified
protected maintenance transaction and installed rehearsal are still required
before using this comparison for an actual upgrade. Ordinary current-schema
verification also validates retained private fee/envelope/invoice/delivery
correlations and refuses overlap with public feed/payment accounting.

## Verification boundary

Tests use real schemas/native SQLite, committed WAL, private ownership, pending
physical UUIDs, signed bytes, original execution claims, failed holds and cost
ACKs. A root fixture creates disposable four-UID-owned files and must emit its
positive completion marker inside an isolated CI network namespace. It uses a
test-only writer-state observer; the production systemctl parser is tested
separately. This is not installed four-user service, live provider, household or
restore-adoption qualification. No real payment/post/feeding is submitted.
