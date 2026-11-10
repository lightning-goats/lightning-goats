# Explicit failed-payment cost reconciliation

Failed outgoing payments keep their full gross reservation held by default.
A provider execution status is not a refund certificate: the documented
[Strike payment response](https://docs.strike.me/api/get-payment-by-id/) describes
execution state and payment/fee amounts, not a separate final-refund attestation.
This implementation therefore does not infer a refund from `FAILED`, a timeout,
a 404, an expired quote or a zero fee. The original verified payment and member
result stay Failed; no received-treat announcement is created.

## Financial authority and evidence

Only a separate protected operator review can reclassify a finalized failed
hold. `lightning-goatsctl failed-cost-template` reads the original retained row,
manifest, execution claim, epoch, account and invoice/payment identities. Its
output deliberately has a null cost/evidence hash, empty review fields and false
review acknowledgement; it cannot authorize anything.

An operator reviews the final debit/refund using the provider receipt, statement
or support evidence, supplies the actual whole-sat cost, references that evidence
and explicitly acknowledges the review. The approval and matching evidence must
both be bounded private regular files with canonical root-owned paths and protected
root-owned ancestors. Neither runtime nor payout UID can create or replace them.
The evidence hash commits exact bytes; it does not automatically authenticate or
interpret the statement. This is an explicit operator financial attestation,
**not** a claim that a payment API proves a refund. Missing/unclear evidence leaves
the hold unchanged. Actual cost may be zero through the original gross amount;
any higher/disputed loss remains outside this automatic reclassification path.

The typed approval is not deserializable from HTTP, a private RPC or a payout
journal DTO. It is constructed only after protected-file/evidence validation.
The command takes no spend key, project signing key, OpenHAB token or webhook
secret. Its optional fresh verification uses the core's existing read credential
with `partner.payment.read` against the fixed production Strike origin.

## Transaction and exact recovery

Preview is the default. It validates retained correlation without a provider call
or financial write. Apply requires an explicit `--apply`, a current approval with
at most a one-day window, and a fresh independent read of the *same original*
failed payment. The provider state, amounts and original binding must still agree
with core's verified failure. HTTP runs outside the committing transaction.

The immediate transaction rechecks the complete original packet and final hold,
retained execution claim, current epoch and monotone budget clock. It appends one
immutable cost receipt keyed by reservation/member and globally unique review ID.
The receipt commits the exact decision, original header/row hashes and verification
time. An exact retry returns that receipt without another provider read or changing
time. A changed amount/evidence/identity is rejected. A failed insert rolls back
all accounting and clock changes; it cannot partially release a hold.

Original outcome rows/finals, payment/quote IDs, source consumption and payout
acknowledgments remain immutable. New cost rows replace only the corresponding
held gross in budget calculations and count the reviewed actual cost as spent
for the next 24 hours from reconciliation. They do not mint funds, make a grant
executable again, reuse its consumed receives, change wallet balance or alter the
operator's ceilings/sweep policy. Feed-credit/debits and physical authority are
untouched. No public/member/overlay message is emitted by a cost review.

Core returns a cost receipt only through a read-only, payout-role private RPC.
There is no write/release RPC, and the intake/browser operator cannot call the
read method. The separate payout journal validates the core peer, exact original
manifest/failed member and final ACK before appending its own immutable receipt.
Until that ACK commits, it conservatively retains its original full hold. A lost
ACK/restart retries the same receipt; local rolling spend dates from ACK acceptance,
which can be later than the core review. No new invoice, quote, execute or signing
operation is involved. `cyberherd-payoutd --sync-failed-cost` opens the existing
journal/config and core socket without loading a sender or signing credential.

Recovery opens require the existing private database and an exact successful
migration version/checksum history matching the installed reviewed binary. They
cannot create an empty journal, add tables, migrate an older snapshot or quietly
change the payout binding. Normal reviewed installation remains responsible for
schema upgrades. Opening the SQLx connection can create normal WAL/SHM bookkeeping;
the preview/recovery-open path does not change schema or paid records.

## Operator procedure — after installation qualification only

No command below authorizes a production install or creates approval/evidence.
Use the matching reviewed core/payout binaries, configuration and existing journal
schemas. Keep review documents and statements private. All identifiers below are
operator-selected original values, never guessed from a balance or a Nostr note.

1. Disable/stop payout execution, retain consistent recovery evidence and identify
   the original finalized failed reservation/member. The optional
   `deploy/cyberherd/failed-cost-review@.service.example` conflicts with the payout
   unit and remains active after review. An explicit start of the conflicting
   payout unit stops the guard; the guard is not a persistent execution fence.
   Keep execution disabled in the protected policy throughout review and do not
   restart it until both journals agree. The unit is not installed or started
   automatically.
2. As the core runtime UID, obtain an unapproved template:

   ```sh
   sudo -u lightning-goats /usr/local/bin/lightning-goatsctl \
     --config /etc/lightning-goats/config.toml \
     failed-cost-template --reservation ORIGINAL_RESERVATION_UUID \
     --member ORIGINAL_MEMBER_HEX
   ```

3. Review final provider debit/refund evidence. Place the exact evidence and edited
   approval under a protected root-owned directory, for example
   `/etc/lightning-goats/cost-reviews/`. Files must be root-owned, core-readable,
   non-writable by runtime/deploy accounts and not readable by other users
   (typically root:lightning-goats, mode 0640). Fill the actual cost, exact SHA256
   evidence digest, statement reference/reason and explicit review acknowledgement;
   preserve all original correlation fields. Choose the review's valid time window.
4. Preview as core UID; this loads no API key:

   ```sh
   sudo -u lightning-goats /usr/local/bin/lightning-goatsctl \
     --config /etc/lightning-goats/config.toml \
     reconcile-failed-cost \
     --approval /etc/lightning-goats/cost-reviews/REVIEW_ID.json \
     --evidence /etc/lightning-goats/cost-reviews/REVIEW_ID.evidence
   ```

5. The operator explicitly applies the reviewed decision. A separately reviewed
   installation of the optional guard unit can provide the core read credential
   through systemd and stop the payout unit first:

   ```sh
   sudo systemctl start failed-cost-review@REVIEW_ID.service
   ```

   Without that unit, an equivalent protected one-shot must provide only the core
   read credential and invoke the same command with `--apply`. Never paste a key
   into argv or add a spending credential. The command never resends the payment.
   A new review still needs current approval time/provider checks; an already
   committed exact retry recovers its original receipt without loading a key.
6. Synchronize the original member's core receipt as the separate payout UID:

   ```sh
   sudo -u cyberherd-payout /usr/local/bin/cyberherd-payoutd \
     --config /etc/cyberherd-payout/config.toml \
     --sync-failed-cost --grant ORIGINAL_RESERVATION_UUID \
     --member ORIGINAL_MEMBER_HEX
   ```

   This works while the worker is disabled and reads no sender key. Check both
   private reports and the original Failed result. Stop the review guard and
   restart execution only as a separate explicit operator action after resolution.
   Never erase an outcome, execute intent, source receipt or cost ACK to retry.

## Source verification and remaining scope

The [485-test source checkpoint](../testing/cyberherd-failed-cost-checkpoint-20261110.md)
records completed source, root-ownership, CLI/browser and inactive artifact checks.

Temporary-ledger/provider/private-IPC fixtures exercise explicit cost/release,
rolling-window timing, wrong scope, pending/unfinalized sources, expired reviews,
insert rollback, immutable retries, independent journal ACK loss and role denial.
A native root-owned file fixture is run explicitly in a disposable CI network
namespace, in addition to ordinary unprivileged rejection checks. Recovery schema
checks refuse missing/empty/old/private-path violations. These are not a live
financial review, provider refund qualification or new-role installation evidence.

Whole-stack backup/consistent restore, retention operations,
installed UID/network/helper/provider/signer/engine qualification and explicit
activation remain required by the complete CyberHerd plan.
The subsequent [SQLite checkpoint](../testing/cyberherd-sqlite-checkpoint-20261110.md)
resolves the dependency-audit finding in source; it does not patch installed programs.

The legacy `deploy/scripts/restore-stores.py` supports the pre-CyberHerd core and
gateway pair only. It now rejects a core containing CyberHerd financial schema
before copying or rotating its epoch, including an uninitialized new schema.
An empty funding table does not prove that no independent consumer has bound the
epoch. Four offline fixtures preserve its legacy behavior and prove this refusal;
they do not qualify multi-journal restore or automatic resumption.
