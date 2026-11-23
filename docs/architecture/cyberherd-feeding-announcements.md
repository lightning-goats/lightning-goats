# Confirmed-feeding announcements

The standalone port keeps one historical `feeder_trigger` announcement per
triggering payment, with one standalone kind-1 note, one authenticated active
stream kind-1311 copy when available, and one human-name overlay text copy. It
adds the editable `cyber_herd_feeder_score` summary to the same bodies: frozen
active member count, total contribution points and configured member allocation.
There is no second roster-summary post or invented joining-member thread.

## Explicit presentation handoff

The receive core remains the financial and physical authority. Optional
`core_bridge.delegated_feeder_messages`, default false, requires its root-pinned
`reward_policy.owner`; monetary limits may remain zero. Startup applies that
presentation policy before financial workers begin. New confirmed feed events
freeze the owner and the original triggering credit identity/amount in their
existing durable payload. A later payment during dispatch cannot steal that
attribution. No account amount, threshold, safety cap, ledger/debit policy,
provider credential or feeder request semantics changes.

When enabled, the normal core renderer retains the original confirmed-feeding
overlay event and physical effect, but produces neither scrolling text nor a
kind-1 note for that event. The private CyberHerd producer becomes its sole text/
Nostr presenter. The core food confirmation/debit never waits for membership,
discovery, templates, the new process or remote signing. A failed presentation
policy lookup falls back to normal core presentation rather than blocking food
confirmation. A disabled or unavailable delegated presenter leaves notices pending;
it must be qualified before the operator selects the handoff. Existing events and
signed outbox bytes are never rewritten. Disabling delegation affects future
confirmations; already delegated notices retain their original owner/destinations.

The core export reconstructs an optional presentation proof from the actual
`feed_event_sources` binding, confirmed UUID, exact matching feed debit and its
prefix ledger balance. It verifies the triggering FIFO boundary credit and
hashes the original source key with the core epoch/project. Raw provider/grant
identifiers are not exposed as public tags. A single payment covering multiple
thresholds has one presentation identity, while each confirmed UUID still has its
own financial/reward barrier. Distinct equally sized payments remain distinct.
Bounded missing attribution uses neutral confirmed-feeding wording and a
UUID-scoped identity; it never substitutes the threshold as a payment amount.
That optional fallback uses editable `feeder_confirmed_unattributed` wording;
its deletion is retained rather than restoring hardcoded text.

## Frozen membership and destinations

Ordered intake still classifies payments before freezing a feeding barrier.
Announcement generation uses that immutable roster/settings/points snapshot,
including payments processed before confirmation, rather than current later
scores. It performs no payout or core financial mutation. After successful target
discovery, it chooses one authenticated current stream when queuing the notice;
failed discovery retains the barrier, and a successful projection with no live
stream produces the regular note only. Subsequent stream changes cannot retarget
that queued message. Ended streams are not eligible for feeder chat copies.

`feeding_announcements` records a durable decision for every barrier. The first
barrier for a triggering credit freezes the body/summary and destination context;
subsequent confirmations for that credit reuse its announcement decision. Each
still remains independently available to confirmed-feeding reward planning.
Deleting `feeder_trigger` opts out for that payment. Deleting the optional score
category removes only the summary. Custom text, deletion tombstones, optional
goat naming, the original legacy export and uploaded artwork are preserved.
The displayed amount is the original triggering payment, never the feed debit.

A typed feeding-broadcast branch reuses the exact delivery outbox, leases,
unsigned fields, native signatures and real relay-OK receipts. Its regular note
has no fabricated `e` root, `p` member or reply; its chat has the frozen stable
stream `a` root and no joining reply. Member conversations and their mandatory
joining receipts remain unchanged. Broadcasts cannot enter member branches.
Frozen signed retries keep their bytes/event IDs. A project-specific source-hash
tag keeps distinct payments distinct even if wording and timestamps coincide. Public-mode permission is
refreshed before signing and publishing. Confirmed food can still be announced if
the override changes afterward; member updates retain their existing Ready gate.
Canary/shadow modes cannot publish these notices to Nostr.

## Overlay authority

The private intake-only `FeedingOverlay` method requires the exact core epoch,
root-pinned project, actual confirmed UUID and reconstructed original proof.
An ordinary receipt, funded balance, arbitrary event named `feeder_confirmed`,
uncertain result or forged UUID is insufficient. It appends only a bounded
`cyberherd_message` text/artwork event, never another food confirmation, credit,
debit, physical command or core Nostr job. Allowed artwork paths remain the five
existing goat image paths. A durable per-source receipt makes a lost ACK retry
return the same WebSocket sequence; changed bytes are rejected.

Feeding-overlay delivery runs independently of member overlay delivery, so a
paused member thread cannot block a true food notice. It remains available during
overlay-only canary staging without a signer credential or joining receipt. The
original core confirmation is the only event that runs the physical feeding
animation; the later enriched text does not trigger it again. The typed member
import still prohibits feed/payment/reset categories.

## Installation and verification boundary

The handoff requires matching updated core/intake code, the configured private
UID/group/owner/epoch boundary, enabled membership messaging, selected activation
cursors and qualified overlay/public signer paths before opting in. Upgrade
intake compatibility before enabling the core flag. The default remains false;
this source does not activate any production component or financial operation.
Gateway/owner safety and ambiguous-outcome recovery remain authoritative.

Required regressions cover original payment attribution with an interleaved
later credit, frozen scores and old stream roots, one note/chat/overlay per
multi-feeding payment while retaining each reward barrier, exact restart/lost-ACK
retries, native private owner/epoch/UUID/role rejection, no extra food/debit,
template opt-out, absence of a live stream, legacy nondelegated behavior,
nonpublic mode and funded/uncertain outcomes. Installed provider/household-client
qualification and the remaining full-port operational/interface work are separate.

## Zap attribution

Zap-specific payment and confirmed-feeding copy uses the signed sender from the
exact invoice-bound NIP-57 request and the authoritative settled amount. Comments,
wallet labels, nearby payments and matching amounts never identify the donor.
Nostr copy uses the existing `nostr:npub…` profile mention and a corresponding `p`
tag. Anonymous requests use a generic anonymous label with no sender mention or
recipient tag. Overlay copy uses the sender's frozen roster name when available,
otherwise a shortened public identifier. It never resolves identity from a name.

The core derives this presentation context read-only from the actual settled
credit; feeding attribution follows the actual consumed-credit boundary. The
separate presenter correlates retained authenticated payment inputs to the
core's epoch/owner/source-key announcement hash before the frozen feeding barrier.
A later payment during dispatch cannot steal the original donor, even at the
same amount. Existing proof and financial wire schemas remain unchanged.

`feeder_trigger_zap` is a separately editable, deterministically selected template
pool. The broader `feeder_trigger` opt-out also suppresses zap-feeding notices.
Seeding retains custom edits, variant tombstones and disabled categories.
Generic and goat-fact variants preserve optional goat naming; artwork appears
only for a goat actually mentioned by the selected variant. The same frozen
message/outbox retains one announcement per triggering payment and exact signed
retry bytes. Previously frozen generic announcements are not retroactively
rewritten or given new recipients.

The independent `messaging.stream_chat` setting continues to govern every
CyberHerd chat destination. The operator disabled it on 2026-11-14; this feature
neither reenables it nor sends informational/weather events to Nostr. Source
verification and deployment evidence belong in the
[zap messaging checkpoint](../testing/zap-payment-messaging-checkpoint-20261114.md).
