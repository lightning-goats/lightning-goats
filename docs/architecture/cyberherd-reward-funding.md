# Confirmed-feeding funding and reward reservations

This is core-owned provenance and budget bookkeeping for the
[full CyberHerd port](../planning/cyberherd-implementation-plan.md).
It does not submit payments or activate a payout worker. The receive daemon
retains its receive-only Strike credential and the existing physical owner.

## Activation and history

Migration 0016 adds empty funding/reservation tables without rewriting any
settlement, credit, debit, event, invoice, feeder UUID or signed message. Tracking
is inactive until the protected core bridge has an explicit root `reward_policy`.
Bridge setup initializes it transactionally before accepting reservation requests.
A setup failure withholds the optional bridge; it does not restart core workers.

Initialization replays existing ledger deltas in order, using FIFO only to
establish the remaining credit lots. It records the historical ledger watermark
and never creates funding snapshots or reward opportunities for old debits.
Retrying initialization is idempotent. A failed bootstrap rolls back its marker
and all partial lots. Existing feed credit and event sequences remain unchanged.

Once tracking is initialized, future confirmations continue accounting for lots
even if the bridge/reward policy is later disabled. Removing a policy cannot
make already consumed funds available again. A post-activation debit missing its
atomic funding record requires recovery rather than reconstructed reward history.
Changing or restoring an epoch/cursor still requires explicit operator recovery.

## Real and unavailable funding

A lot is tied to its actual positive ledger entry. Strike BTC funding additionally
requires the native settlement, asset receipt, valuation/allocation and grant
provenance, matching provider/source, hash, address and pool. Only the supported
`herd` pool qualifies. Its immutable fingerprint is checked again before consuming
the lot. A payment-shaped event or an uncorrelated ledger row is insufficient.

Operator grants, other providers/pools and preserved unsupported asset/legacy
credit remain unavailable for Strike rewards. They still represent their existing
feed credit; no currency conversion or activation of a held payment rail occurs.
An authoritative actual BTC P2P amount can fund rewards even when it did not
qualify for a native invoice-paid zap receipt. Membership qualification remains
separate from whether real money funded a feeding.

Lot classification proves receipt provenance, not that the same money is still
in the provider account. Sweeps and unrelated account activity are not reversed.
The outgoing worker must still verify its account/credential binding, current
provider availability, limits and authoritative payment outcome. No assumed
wallet balance or operator-funded virtual grant authorizes a transfer.

## Confirmation and immutable proof

A confirmed feeding consumes its threshold from the oldest remaining lots in
the same transaction as the existing UUID confirmation, debit and event binding.
Intent, receipt-only acknowledgement, unknown/refused/not-dispatched outcomes and
funded balance alone consume no lots and create no reward opportunity.

The stored funding snapshot contains its actual debit, total amount, Strike BTC
portion and manifest hash. Ordered lot IDs, consumed amounts, classifications
and source fingerprints contribute to that hash. Readback verifies the manifest,
remaining positive-credit links and the confirmed UUID/debit. Parts/snapshots are
immutable; lot amounts can only decrease. Their remaining sum matches durable
feed credit. A failed funding write rolls back the confirmation/debit and retains
the original unresolved UUID; recovery never substitutes a fresh physical action.

For example, 300 virtual sats followed by 1,000 real Strike sats fund a 1,000-sat
feeding using 300 virtual and 700 real sats. At a 10% maximum allocation, its
reward ceiling is 70 sats. The remaining 300 real sats can fund a later feeding;
no source is counted twice. A virtual-only feeding has a zero reward ceiling.

## Optional reservation capability

Private protocol version 2 carries optional funding on confirmed-feeding proof.
The ordered consumer retains it in the same immutable membership barrier. Old
untracked feeds carry no funding capability. The core and consumer must be
upgraded together; a version mismatch fails closed without resetting a cursor.
Funding hashes/source details are private IPC/state, never overlay/public payload.

Without `core_bridge.reward_policy`, the bridge remains read-only. With the policy,
the allowed peer may request only a typed reservation for a pinned epoch, owner,
confirmed feeder UUID, exact frozen plan hash and positive integer budget. It
cannot provide SQL, ledger deltas, provider receipts, a gateway URL or feeding
commands. Kernel UID authentication, framing/rate/concurrency/deadline limits
still apply. Shadow mode refuses reservations; canary/active additionally require
the explicit policy. No mode grants a spend credential to this core process.

The root policy sets one owner, a member-percentage ceiling, a maximum per feeding
and `max_sats_per_24h`. Zero remains zero. The percentage applies
only to the real Strike portion; requested net amounts **plus all fee allowances**
must fit within it. The percentage calculation floors to whole sats. All existing
reservations are still outstanding and count against the global limit, including
old/future-dated ones. An unknown or submitted payment cannot age out by creation
time and then spend after its budget was reused. This conservative cap does not
renew automatically each day: authenticated terminal outcomes and recent actual
spending must be accounted for before budgets can be released/aged. That outgoing
integration remains work. Limits are shared across owners/configuration changes.

One confirmed UUID has at most one immutable reservation. Exact retries return
the same ID; a changed owner, amount, plan or epoch cannot create another one.
The root policy is frozen with the reservation for audit/retry readback; lowering
limits does not rewrite an existing allocation or release its outstanding budget.
Reservations and replay records are never deleted to release capacity. The client
returns a non-deserializable authenticated capability after checking the peer and
matching response. A persisted DTO alone is not sufficient authority for a future
payout worker; it will need authenticated core readback/reconciliation.

A reservation leaves feed credit untouched and is not a submitted/settled payout
or evidence of a recipient treat. Unaffordable/skipped shares and unused fee
allowance stay operations; no member carry-forward balance is introduced. Frozen
eligible roster/fees, plan persistence and outgoing reconciliation remain work.

## Installation and rollback boundary

The policy example is commented out with invalid owner/zero limits. No source
test or archive check enables it on a host. Qualify bootstrap on a backed-up copy,
pending UUIDs, core/consumer versions, root owner/limits, separate identities,
account binding, fee policy and the payout worker before explicit activation.

Disabling reservations is distinct from downgrading schema. Older SQLx binaries
may reject migration 0016. Use a compatible forward revert/disabled policy while
preserving current receipts, UUIDs, lots and reservations; never restore an older
paid ledger to undo a transfer or regain a budget. Provider spend, receipt
publication, household signer grants and physical qualification remain separate.
