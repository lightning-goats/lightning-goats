# Independent payout funding journal

The separate payout store advances requirement 8 of the
[complete CyberHerd port](../planning/cyberherd-implementation-plan.md).
It admits a frozen plan only with the non-deserializable
[qualified funding capability](cyberherd-payout-authority.md). It has no public
ingress, provider execution, signer, feeder command or announcement method.

## Binding and admission

`payout-migrations/` belongs only to the separate payout identity's database.
Opening a core or membership database is rejected before adding payout tables.
The store verifies filesystem SQLite with WAL, FULL synchronous and foreign keys.
An immutable binding pins core UID, core epoch, owner, production environment,
operator-labelled account and credential fingerprint. Changed identity requires
explicit recovery; re-opening with a different label cannot clear consumed funds.

Admission independently checks the reservation and plan hash, actual confirmed
funding/threshold, root policy binding, percentages and per-feeding ceiling.
It rechecks positive exact whole-sat net/fee/gross amounts, receiver minima/maxima,
unique members/addresses/callbacks, operational-address and owner exclusions,
signed-profile identity references and independently configured maximum fees.
An authenticated plan hash commits the intake's roster decisions; this does not
claim the payout store independently re-fetches every historical membership event.

One transaction freezes the plan, private qualification evidence, actual source
parts, payable destination/amount rows and one admission journal entry. A write
lock precedes capacity/source reads. Concurrent/restarted identical admission
returns the same grant; changed evidence cannot turn a retry into another grant.
Skipped shares create no payable row or carried member balance.

## Retained source consumption and capacity

Provider receive identity, request identity and total received sats are retained
independently of the core's local ledger IDs. Each confirmed grant consumes only
its actual slice of a receive. Distinct feedings may consume disjoint portions
of one larger verified receive; cumulative consumption cannot exceed that receive.
Changing local ledger IDs or replaying a restored core record cannot reuse an
already consumed slice. A failure rolls back grant, source, row and journal writes.

Every admitted gross allocation remains held against the independent root budget.
No grant ages out after 24 hours merely because it was created long ago. This is
deliberately conservative until authoritative terminal-outcome accounting exists.
The later outcome protocol must release unused funds and retain actual completed
spending for the rolling window; that protocol is still required for the full port.
An admission clock high-water mark rejects backward time for new grants. Reducing
policy does not delete outstanding grants or prevent readonly recovery inspection.

Application SQL rejects edits/deletions of immutable grants, payees, source parts
and journal entries, source-counter reduction and binding changes. These guards
do not protect against an administrator replacing the whole database. Never restore
an older payout database and resume transfers automatically: restore/recovery must
reconcile actual provider outcomes and retained intents first. Keep this private
store in the operator's consistent backup and recovery procedure.

## Qualification boundaries

Tests use temporary separate databases, actual Unix peer framing and loopback
provider fixtures. They exercise concurrent/restarted admission, disjoint source
slices, overconsumption under changed ledger IDs, account/credential/epoch rebinding,
whole-transaction rollback, held capacity after a day, clock reversal and payee
exclusions. Private test constructors additionally model adversarial restored-core
evidence; production callers cannot deserialize it into qualified funding.

This is library source, not an installed payout service. Quote/execute intents,
strict outgoing invoice and NIP-57 handling, authenticated terminal budget outcomes,
treat/thread messages, worker deployment and financial activation remain unfinished.
The public receive daemon remains receive-only; no runtime credential changes are
made by this implementation.
