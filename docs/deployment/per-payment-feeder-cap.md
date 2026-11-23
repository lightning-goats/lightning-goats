# Three-feed payment cap and one announcement

The operator requires at most three physical feedings from any payment, then no
remaining feeder credit from that capped batch. One payment may generate only
one feeding announcement. Separate physical confirmations remain separate durable
financial events and reward opportunities; repeated confirmations are not new
payments. This corrects the uncapped queue and repeated announcement defects.

A payment and its full receipt value remain in the paid journal. With a configured
threshold, new payment credit can fund at most three additional thresholds after
the already saved partial threshold. Excess is retired through a distinct audited
`FEED_CREDIT_RETIRED` entry, never a physical `FEED_DEBIT` or refund. For example,
70 saved sats plus a 34,031-sat payment allocate 3,000 sats to three feedings and
retire 31,101 sats. Only actual confirmed delivery consumes those three thresholds.
Smaller payments retain ordinary progress/remainders. Later payments retain their
own capped credit; finishing one batch must not erase another donor's funds.

Retirement consumes the exact original funding lot, without adding any physical
feed or reward proof. Reopen/recovery checks retain original receipt provenance,
retirement linkage and FIFO balance. Provider identity, settlement and full native
receipt values are unchanged. CLI grants use the same allocation rule.

The first actual confirmation binds one durable announcement source to its paid
credit identity. Later confirmations update credit and accounting without another
Nostr/overlay announcement. Older first confirmations are indexed on migration;
unsent duplicate signed outbox rows are quarantined with retained suppression
audit records. Their signed bytes and all published history remain intact.

## Containment and repair

Use the protected containment helper only with the receive service stopped. It
preserves the complete original configuration before selecting shadow mode and
suspending the optional contact route (which requires active mode). This restores
payment acceptance while holding feeding and Nostr publication. It never opens
journals, calls a provider/gateway or restores active mode. Install it as a
root-owned non-writable file and independently verify its digest before running.
The operator's local feeder override stays under their control.

Before schema upgrade/repair, quiesce all writers and take/verify a current native
four-journal backup using the original and selected schema tooling. Do not use a
new CLI on an unmigrated live ledger merely to preview it. The installed new CLI
uses existing-schema recovery mode for the explicit retirement command and
requires root, protected configuration, an actually stopped core and the explicit
stopped-writer acknowledgment:

```text
lightning-goatsctl --config PROTECTED_CONFIG retire-feed-credit
  --id STABLE_REPAIR_ID --credit-entry-id ORIGINAL_CREDIT_ROW
  --expected-sats EXACT_UNSPENT_SOURCE_AMOUNT --writers-stopped [--apply]
```

Preview is rollback-only. Application retires exactly the reviewed original lot;
changed amounts, an unresolved physical attempt or a changed retry fail closed.
Replays preserve the same audit identity and cannot retire twice. In the reported
incident, already confirmed over-delivery remains truthful history; retiring the
remaining credit must not generate more feedings or claim fewer occurred.

Restore the full protected original active/contact configuration only after the
verified fix and exact backlog retirement are applied. Retain any local override
selected by the operator. Verify public status, receipt preservation, the corrected
queue, unchanged actual feeding count and absence of repeated announcements.

## Qualified additive upgrade

The explicit `--feeder-cap` preparation and maintenance selectors require the
contained shadow configuration without contact and seal a distinct transaction
kind. They accept only the compiled exact schema-24 source; the native comparison
allows migration 25's four new tables and its exact derived first-announcement
and suppression rows. All original row IDs, signed bytes, schema objects, and
migration-history prefixes must remain identical across all four journals.
Policy and retirement tables must still be empty at this comparison boundary.
The older contact source remains exact schema 23; its current qualified target
includes both additive migrations 24 and 25, with the same retained-row checks.
Neither comparison authorizes service activation or erases the backlog.

## Production repair verified (2026-11-11)

The cap and announcement fix was deployed from qualified source
`be6ad767829b7d2693b4f1030aa3952f8b7d2555`. The current-schema offline
retirement CLI was corrected and separately qualified at
`66c817b0cf93bfac30e5755adf416322345457a8`; all ten workflows passed,
including six mandatory actual-root native fixtures. Its installed executable
preserves the original ownership, permissions and security attributes.

The first rollback-only retirement preview refused the root/runtime ownership
mismatch before opening the journal. The corrected root-only adapter validates
existing runtime-owned files, never creates or migrates the main journal, closes
SQLite on success and failure, and returns any retained private WAL sidecars to
the original runtime owner. Ordinary runtime recovery still rejects root. The
CLI also refuses retirement while the core is running.

After the independently verified four-journal backup and additive migration,
exactly 24,101 sats of the offending donation's unused credit were retired through
one audited non-feeding adjustment. The original donation and all ten actual
confirmed feedings remain in history; no additional feeding, payment, refund or
reward was created. The verified post-repair backup and original transaction
records are retained privately on the host. Completing a migrated transaction
must never rerun the older source-schema migration against current paid state.

The original active/contact configuration and payment/overlay service were
restored. Public status and both primary/alias herd LNURL metadata were checked;
the overlay reconnected with zero credit and zero queued feedings. Retained event
replay confirmed the original ten feeding events plus one retirement. The local
operator override remained on throughout. CyberHerd payout and signer services
remain masked with their existing zero financial ceilings. Verification created
no real Lightning payment or physical feeding.
