# CyberHerd intake and ordered membership worker

`cyberherdd` connects the private core bridge, authenticated target/profile
discovery and separate membership store. It has no Strike key, Nostr signer,
core database handle, OpenHAB credential or feeding command. It does not publish
messages or pay rewards. Those capabilities remain part of the
[full implementation plan](../planning/cyberherd-implementation-plan.md).

## Activation and receiving authority

Root configuration pins the core UID, durable epoch, explicit starting cursor,
credit pool and receive-user/native-payee mappings for one project roster.
Initialization is idempotent only for identical configuration. A changed identity,
epoch or activation boundary requires explicit recovery; none is silently reset.
The example is disabled and contains deliberately invalid identity placeholders.
`cyberherdd --config PATH --check` validates without opening a database, querying
a relay or starting services. Normal startup additionally requires protected
root-owned configuration and parents, a separate non-root runtime UID and a
canonical private state directory. A Linux kernel socket lock prevents concurrent
processes from owning that state and disappears automatically after a crash.

Existing membership settings and template edits are never overwritten by restart;
configuration supplies initial settings, while the durable store remains their
operating source of truth. Operator editing interfaces remain unfinished.

Core input accepts only the client's non-deserializable `AuthenticatedBatch`.
Each epoch/sequence retains its exact envelope hash. Separately, provider/source
identity binds immutable financial facts independently of local row IDs and
Nostr request IDs. The same source cannot change its amount, payee, address,
settlement time or invoice/request bytes. Distinct settled invoices with the same
signed zap request count independently. An identical domain source encountered
again does not create new membership work.

The receiving cursor, envelope receipts and queued work commit atomically. Failed
validation, a gap, changed epoch, storage error or full queue leaves the upstream
cursor unchanged. Virtual operator credit and unrelated core events retain
envelope receipts but never become zaps or real reward funding. Ordinary receives
stay financially valid and complete without creating membership points.

## One durable observation order

Raw signed social staging and authenticated core ingestion append to the same
ordered queue. This records first observation, rather than trusting payer/event
timestamps to reorder already observed work. One blocked head remains before a
feeding; missing profiles, unknown safety or incomplete discovery cannot silently
skip it. A later-arriving backdated social event stays after that feeding.

Applied domain receipts survive a crash before queue completion. A retry uses the
same frozen profile, increment and source, even if discovery/profile services are
currently unavailable. A completed interaction cannot score again. Deferred zaps
retain active-target-only eligibility, including when a missing profile prevented
the first membership transaction. An active member's ordinary project-note zap
still has broader eligibility when first processed while ready.

A pause/unknown observation restricts all already queued payments, including
those behind a blocked head. Missing profiles cannot undo that restriction.
Work whose authoritative occurrence predates a closed cycle can receive a
minimal discard receipt without profile/target lookup; unavailable discovery
still cannot discard work from the current cycle.

At a confirmed feeding head, a transaction freezes the membership snapshot and
marks that input complete together. The owner/UUID barrier is immutable. This is
**not a funded payout plan**: current payout eligibility, real funds, fees, minima
and reservation still require separate planning. It does not authorize a transfer
or additional physical action. Feeding proof remains the core's correlated UUID
and debit; no copied JSON or threshold-only notification can supply it.

Private protocol version 2 also retains the core's optional
[consumed-funding proof](cyberherd-reward-funding.md) in that barrier. Its client
offers a root-bounded reservation capability, disabled without explicit core
policy. Optional [reward planning](cyberherd-reward-planning.md) now runs independent
bounded rounds, freezing eligible plans before requesting their reservation.
There is no provider-spend, signer or publication capability in this worker.

## Discovery and recovery

Each runtime round catches up the optional Mountain-time reset, receives a bounded
core page and refreshes signed targets before processing work. Queries separately
cover project announcements, project streams and host `p` tags; references and
profiles use bounded ID/author queries. Raw referenced streams gain no alias
authority until project signature, host proof or fixed-provider verification
accepts them. Signed negative profiles supersede older payout addresses. Missing
profile lookup remains retryable and does not manufacture an address removal.

The root-selected relay completion quorum defines availability. Saturation always
makes discovery unavailable. No failed/incomplete query proves that a cached target
is stale. Five-minute social backfill windows retain a durable overlap and shrink
on saturation; a saturated second is held for recovery, never skipped. A changed
target set gets its own backfill identity. Raw staging precedes profile lookup;
cursor advancement follows successful durable staging. Count limits on social
and combined ingress backpressure upstream intake while allowing existing work
to drain. An unrecoverable saturated second needs further bounded pagination or
operator review; it must not be advanced by claiming missing data was processed.

The worker reprojects cached heads at processing time and reads fresh core safety
after network work. Unknown/unavailable state cannot resume membership. The core
continues settlement, credits and physical feeding independently of this worker.
It never fabricates a daily announcement. Publishing that note remains external.

## Operations still to qualify

The systemd and configuration examples are preparation, not installed services.
The existing core release package is unchanged; build this separate binary with
`cargo build --locked --bin cyberherdd`. Installation needs reviewed cross-UID
socket/group access, protected binary/config/state, actual public relay and DNS
checks, private-network exclusion, stop/restart/restore tests and an explicitly
selected activation epoch/cutoff. Do not borrow the core runtime group or grant
this identity trusted-network reach.

No receiving-state restore may erase replay receipts or reset a cursor against a
new core epoch. Source-level integration tests use synthetic signed requests,
temporary core/membership databases and private Unix sockets. They do not prove
the provisioned cross-UID runtime or authorize production membership/payouts.

Outgoing provider reconciliation,
note/chat/overlay outboxes, lifecycle notices, public roster/OBS and
authenticated operator interfaces remain unfinished. The known SQLite audit
finding remains open; private state permissions mitigate its local-file condition
without claiming that the dependency is patched.

## Live reaction catch-up correction — 2026-11-10

The first overlay-stage reaction test exposed delayed social intake: recovery
scanned five-minute slices from cycle start, and a refreshed stream version or
alias changed the combined cursor identity. Recent eligible reactions could
therefore wait behind history or repeated scan restarts.

The correction queries a bounded recent window independently, then recovers the
oldest pending per-event/per-stream-coordinate scopes. A quiet historical day can
complete in one bounded query; saturation bisects rather than skipping records.
Only complete unsaturated windows advance covered scope cursors. A recent scan
never advances a cursor across an unscanned gap. New stream versions/aliases gain
their own recovery scope without resetting the daily note or prior stream scopes.
Legacy cursor rows and all observed/ingress/award receipts remain retained.

This applies to every eligible user's reaction/repost, including earlier events
from the current cycle. Native signatures, authenticated targets, latest signed
profile/Lightning-address eligibility, pause rules, capacity and once-per-cycle
awards remain unchanged. No NIP-05 requirement, synthetic membership, financial
state reset, public publication or payout capability is added. New source awaits
exact Rust/CI/installed qualification and bounded worker replacement.
