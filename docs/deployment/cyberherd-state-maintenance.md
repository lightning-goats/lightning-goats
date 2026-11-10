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

## Verification boundary

Tests use real schemas/native SQLite, committed WAL, private ownership, pending
physical UUIDs, signed bytes, original execution claims, failed holds and cost
ACKs. A root fixture creates disposable four-UID-owned files and must emit its
positive completion marker inside an isolated CI network namespace. It uses a
test-only writer-state observer; the production systemctl parser is tested
separately. This is not installed four-user service, live provider, household or
restore-adoption qualification. No real payment/post/feeding is submitted.
