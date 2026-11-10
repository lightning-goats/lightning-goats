# Original payout execution claims and snapshot recovery

Core retains the original reservation/member/quote/invoice binding before the
payout worker may send its first provider execute PATCH. This independent record
closes the payout-only snapshot rollback gap identified in the
[runtime checkpoint](../testing/cyberherd-runtime-checkpoint-20261110.md). It does
not install or enable any financial process.

The kernel-authenticated payout role submits the canonical, natively validated
quote and exact reservation. Core checks the current epoch, confirmed funding,
root account and spending policy, current execution mode and the public Nostr
permission for supported zap recipients. Claims are immutable and retained.
Their total gross amount cannot exceed the reservation; quote IDs, quote-intent
IDs, invoice IDs and invoice payment hashes are globally unique. A reservation
cannot mix provider scopes. Outcome reporting blocks additional execution claims.

The provider adapter requires both the independent payout journal's execute
intent and an opaque fresh core lease for precisely that same request. A DTO
loaded from a file is not such a lease. Execution permission expires after five
seconds and is checked at the network boundary. The legacy funding-page RPC now
always returns execution disabled: an older worker cannot keep spending through
that RPC after core is upgraded. Source qualification remains available.

Before invoice, quote or submission work, the new worker reads core's current
claim and permission together. If core remembers an execution but the restored
payout database lacks its exact original quote, work stops for reconciliation.
If that quote survives, the worker records the retained claim and recovers only
that same quote/payment, including after quote expiry. It cannot replace the
invoice or create a fresh quote because its local execute marker disappeared.
Known payment-ID reads remain available with execution disabled.

Core rejects an unsubmitted attestation for any claimed row, rejects a payment
whose quote differs from its claim, and refuses a final manifest omitting a
claimed member. Checks occur before provider reads and again in the committing
transaction. A committed exact outcome retry remains readable without another
provider call. Partial reports and ambiguous executions keep funds held.

## Recovery scope and operator procedure

The protected payout UID still attests member/plan row correlation. Claims do not
independently rerun membership planning or constrain an attacker already holding
the spend key outside these programs. Whole-core-and-payout snapshot rollback
also cannot be detected solely by two rolled-back databases. It requires retained
external provider evidence and an explicit consistent restore/epoch decision.
Never describe normal process restart tests as proof of that wider recovery.

For a suspected rollback, disable execution in the protected configuration before
starting any worker. Preserve consistent copies of all core, membership, payout
and signer journals and their WAL contents. Inspect the original core execution
state through the authenticated payout RPC. Restore the matching original payout
journal when available; never delete a claim, manufacture a no-execute closure,
or generate replacement invoice/quote IDs to clear a hold. If original evidence
is missing, keep that row held for provider/operator reconciliation. A provider
404 or expired quote does not establish that no payment occurred. Re-enabling
execution remains an explicit operator step after reconciliation.

The fixtures use consistent SQLite `VACUUM INTO` snapshots, independent retained
core state, actual Unix peer checks and synthetic HTTP payments. They exercise a
snapshot missing its quote, a snapshot retaining its original quote but losing
execute/binding records, recovery after actual quote expiry, conflicting or
omitted terminal rows, immutable claim retention and role/policy rejection.
Results must be recorded only after the checks finish on the exact source.

Separate [failed-cost review source](cyberherd-failed-cost-recovery.md) now adds
protected operator decisions and exact core/payout cost receipts. It remains
uninstalled and under source verification. Wider consistent-restore tooling,
installation qualification and live financial activation remain unfinished.
