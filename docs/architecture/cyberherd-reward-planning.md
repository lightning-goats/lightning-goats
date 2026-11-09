# Frozen confirmed-feeding reward plans

This implements the planning portion of requirements 7/8 in the
[full CyberHerd plan](../planning/cyberherd-implementation-plan.md). It adds no
spending credential, invoice callback, provider execute request, feeder action
or public post. The core remains receive-only. No running service is activated.

## Evidence and ordering

Earlier payments/interactions must be classified before the ordered worker commits
a feeding membership barrier. Planning reads that stored barrier; callers cannot
substitute another roster or manufactured feeding DTO. The plan records its hash,
owner, core UID/epoch, UUID, cycle/revision, threshold and consumed-funding hash.
Historical/unfunded barriers and zero allocations close as `no_payout`, without
new profile lookup or retrospective reward authority.

For positive funded allocations, the private `reward_allowance` read requires the
same authenticated peer, owner, explicit root reward policy and feed-enabled mode
as reservations. It revalidates confirmed funding/epoch and returns policy
ceilings and outstanding capacity. It is advisory: atomic reservation repeats
the financial checks. It is not a provider balance or outgoing-payment proof.

Migration 0005 affects only the separate CyberHerd database. Plan insertion is
once per owner/feed UUID. The only update attaches an authenticated reservation,
retaining the original plan/hash. Deletion and reallocation are rejected. A
persisted reservation DTO alone cannot confer the authenticated capability.

## Current eligibility

Scores, awards and membership come from the feeding barrier; later members cannot
enter it. Current signed kind-0 profiles supply addresses/names independently of
historical interaction profiles. Discovery records relay completion and its signed
head. Failed/incomplete/saturated lookup invalidates prior completion; it does not
mean address removal. A signed negative update removes eligibility. A completed
lookup without a signed profile records absence rather than fabricating a profile.
NIP-05 is not required.

Freshness is root-configured. Freeze compares the exact profile/read inside its
transaction after recipient discovery. Changed heads, failed lookup, future or
expired observations hold planning. It cannot reuse an older interaction address.

Eligible active addresses divide the member pool. Owner identity, explicitly
configured operational addresses, case-folded duplicate addresses and identical
canonical callbacks are excluded. Actual LNURL requests preserve local-part case.
Operators must inventory operational aliases; different unknown URLs cannot prove
different wallets. Provider receipt keys/node identities are not member identities.
Later invoice/provider qualification must reject ambiguous/self transfers before
claiming a member treat.

## Amounts and fees

The effective percentage is the smaller of frozen settings and the root ceiling.
The budget is `floor(consumed real sats * percentage / 100)`, capped per feeding.
Virtual/operator credit supplies no real outgoing reward funds.

Existing exact point units/six-decimal largest-remainder shares divide this budget.
Each whole-sat gross share rounds down. Fee allowance is the larger of the explicit
minimum fee and rounded-up basis-point fee. Net is gross less this allowance.
Zero/unaffordable shares, failed recipient lookups and amounts outside exact
recipient millisatoshi limits are skipped, without redistribution or carry-forward.
Skipped rows retain their proposed gross share/reason with zero net/fee spending.

Only payable gross shares (`net + maximum fee`) count toward the reservation. Their
sum cannot exceed the member budget or available root capacity. Capacity shortage
holds the plan rather than shrinking it to temporary capacity. Unused allocation
and rounding remain with operations. `operations_sats` means unallocated confirmed
credit, not an executed transfer or a provider wallet balance.

Fee policy is explicit and bounded. Example values are illustrative, not a claimed
Strike quote or an unverified legacy fee default. Actual outgoing quotes/fees still
require qualification before execution.

## Recipient reads and retries

Discovery follows [LUD-06](https://github.com/lnurl/luds/blob/luds/06.md) and
[NIP-57](https://github.com/nostr-protocol/nips/blob/master/57.md). HTTPS port 443,
bounded DNS/body/deadlines, every-answer public-IP checks, pinned connections,
no environment proxy and no redirects remain mandatory. The 196,608-byte response
bound accommodates the specified thumbnail. Persist only the exact raw metadata
string's SHA256, not embedded images. Future optional metadata values may be any
JSON type. Callback credentials, fragments, private destinations and preselected
payment parameters are rejected.

Advertised Nostr support needs a valid receipt key; malformed support fails rather
than silently downgrading. Absent/false support records ordinary-Lightning fallback.
Member identity remains distinct from provider receipt identity. Future callback
requests must revalidate DNS. Discovery creates/publishes no 9734 or invoice.

Optional `rewards` configuration starts independent bounded rounds in `cyberherdd`:
eight profile filters or eight recipient GETs per round, at most four simultaneous
recipient GETs. Durable staging binds results to a feeding/profile head, allowing
large-roster/restart progress. Failed GETs explicitly skip shares. Raw callbacks or
provider errors are not broadcast. Size profile freshness for roster/relay latency.
Dropping the runtime aborts its planner task.

Plan commit precedes reservation. A restart/lost response retries the exact saved
owner/epoch/UUID/hash/amount before a new allowance; its already committed reservation
may have reduced capacity. Attachment is idempotent and checks core UID, epoch,
funding, owner, hash and amount. A changed root policy still has to pass current
core reservation limits; it does not automatically reallocate the saved plan.

## Remaining outgoing authority

A reserved plan is not an executed payout. The separate payout identity still needs
authenticated core reservation readback, credential/account/environment binding,
signed 9734 freezing before invoice callbacks, exact BOLT11 checks, durable provider
intents/quotes/execute reconciliation and terminal-outcome budget accounting. The
conservative outstanding cap still counts every reservation. Only authoritative
completed payouts may create treat notices. Installation/activation and the full
presentation/lifecycle/interface port remain unfinished.
