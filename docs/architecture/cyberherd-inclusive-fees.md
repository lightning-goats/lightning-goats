# Fees deducted from CyberHerd payouts

The operator selected 10% member allocation, 100 sats total per confirmed feeding,
1,000 sats total per rolling 24 hours, at most 100 sats delivered per payment and
100 new signing requests per hour. Fees have no separate operator ceiling; they
come out of each member's gross share. Gross spending, including fees, remains
inside the feeding and daily ceilings. A 100-sat share with a quoted 2-sat fee
delivers 98 sats. Unaffordable shares are skipped and unused allocation remains
with operations.

Strike's [quote API](https://docs.strike.me/api/create-lightning-payment-quote/)
supplies receiver amount, fee and total-spend fields. Its inclusive amount option
is documented for zero-amount invoices. Existing fixed-amount LNURL/NIP-57
invoices must retain their exact amount and description hash; a fee change cannot
silently alter an already signed invoice or zap request.

The source now contains gross-budget arithmetic and an append-only fee-observation
journal. Fee probing has a distinct opaque response type. An oversized observation
cannot convert to an executable quote: the original invoice, fee allowance,
gross-budget and deadline checks still apply. A private validated decoder restores
retained observations; public callers cannot deserialize an authenticated response.
Journal reads bind evidence to the original provider request, grant, member and
scalar money fields. Discovery requiring a smaller amount atomically retains the
observation and retires only the original never-executed quote. A fitting quote
remains subject to the ordinary reservation and execution authority.

The new payout-only migration preserves all original rows and adds immutable,
retained observations. Successive discovery amounts must strictly decrease;
rounds are bounded to eight and cannot continue after execution or after a fitting
quote. Provider quote identities remain unique and replayed observations must
retain exact bytes. This schema is source-only and has not migrated the live
payout journal. It is not covered by the completed program-only maintenance grant.

Integration remains unfinished. Reduced invoices and matching signed zap requests
need versioned, durable identities without changing old bytes. The scheduler must
recover observations after restart, preserve unknown submissions and original
provider idempotency, enforce the signing-rate ceiling for every new attempt, and
execute only a final quote fitting the frozen gross reservation. Root financial
configuration rendering and qualification must support the aggregate-only fee
policy without using a 100-sat fixed fee floor or a zero-fee assumption. Historical
fixed-fee plans must retain their exact serialization and replay semantics.

Four focused arithmetic tests passed, including exhaustive small-budget cases.
The native journal/provider tests and full Rust/security gates remain pending;
the first local full-library test compilation was deliberately terminated after
heavy paging on the live host. Compilation and integration verification use CI.
No real provider operation, financial activation or live schema change follows
from this source checkpoint. The full twelve-part implementation remains open.
