# Frozen member conversations and exact delivery

This extends the [verified result handoff](cyberherd-verified-results.md) into
optional member note/chat/overlay delivery. It advances requirements 9–10 of the
[full port](../planning/cyberherd-implementation-plan.md); it does not install,
enable or qualify a production messaging or payout service.

## Admission, feeding and payment contexts

Authenticated ingress saves the original admission conversation with membership.
Later increases do not overwrite that associated note/stream. A successful
rejoining starts a new generation. Each note or stable stream address has its own
branch within the generation; all branches still share one membership score.
The confirmed-feeding transaction freezes active member conversation contexts
beside its roster. Plans copy those contexts and their exact score basis before
later interactions or a different live stream can change them.

Outgoing zap wording is selected from the editable `cyber_herd_zap_request`
catalog and frozen in each payable plan row. Deleting the category opts out of
text, not NIP-57. Wordings are bounded before reserving funds. Existing saved
plans/intents without these fields retain their previous serialization/hash and
profile-zap behavior; no history is rewritten.

The payout process may read a joining reference through its existing protected
plan socket. The membership server reads only the original plan's member
conversation and a real retained publication receipt. Native verification checks
the reference's project signature, member tag and kind. The payout journal freezes
the reference with the invoice intent before signing/callback work. Kind 9734
contains the exact editable text and, when available, the joining `e` reference
and its `k` kind, including 1311. If the reference is missing or the read is
unavailable, payment proceeds as a profile zap. This neither downgrades a supported
recipient to ordinary Lightning nor publishes the request to relays.

## Rendering and durable queues

Generation reads each private domain event once and saves body, human-name
overlay copy, target addresses and the relay set with its cursor in one
transaction. Owner/member/source identity is independent of event-type labels
and repeated `msg:` wrappers. Duplicate aliases cannot create another message.
An explicit activation cursor is retained; changing it requires recovery.

The legacy catalog supplies interaction wording, increments, current-score,
capacity, payout breakdown and compact allocation footers. The uploaded export
remains unchanged. A small separate default catalog supplies neutral rejection
and delivery-failure wording rather than making an unsupported provider-specific
claim. Custom edits, category opt-out and deletion tombstones remain authoritative.
Main public names become intentional Nostr references; overlay and allocation
footer names remain plain human names without extra `p` tags. Footer inclusion
is decided from before/after allocations in the membership transaction. Scores,
increments and payout percentages never come from a later mutable roster.

Sub-10-sat incoming payments still change valid membership scores but generate no
payment announcement. Duplicate social awards generate no extra message. Manual
and daily resets, arbitrary categories, information and weather never fall through
to member Nostr publication. Skipped unaffordable shares can queue their frozen
no-treat breakdown; shared or operational recipients cannot create treat claims.

The first accepted interaction establishes a branch's pending joining intent.
Rejected admissions never reserve a member thread. A stored unsigned event or
signature is not a publication receipt. Later replies wait until a relay has
acknowledged the starter's actual signed ID. Treat replies additionally wait for
that real joining receipt, independently of completed money movement. Older
results missing conversation history remain queued until a genuine interaction
receipt can supply a branch; they never fabricate a separate thread anchor.

WebSocket notices have no Nostr reply anchor: a core-verified human-name treat
notice can be delivered once during overlay-only staging or a relay outage while
its public thread reply waits for a joining receipt. It still obeys the independent
confirmed-unpaused overlay gate and never creates a publication receipt.

Unsigned kind/time/content/tags are saved before nak. Native signature checks
require the exact frozen fields. Signed bytes are saved before relay delivery.
Each destination/relay has its own immutable matching-OK receipt; retries use the
same stored signed event. Known completed destinations are excluded before work
selection, while waiting branches rotate instead of starving unrelated work.
Stream chats use the saved stable `a` root and direct joining-message `e` reply.

## Runtime and overlay boundary

`[messaging]` is optional and defaults absent/disabled. With no signer configured,
the daemon can deliver overlay-only member activity. A configured signer loads a
distinct pinned NIP-46 authentication client as the `nostr-client-key` systemd
credential. It rejects the project identity's secret. The household bunker holds
the project key; provision this client's kinds 1/1311 deliberately. The outgoing
9734-only broker is not expanded into an arbitrary-event signing service.

Generation, Nostr delivery and overlay delivery run as separate bounded tasks.
Public signing/publication needs fresh confirmed unpaused status and public mode;
status is refreshed again before publishing after a potentially slow signing call.
Unknown, unavailable, paused or non-public mode cannot produce a joining receipt.
No money, feeder or core financial capability is added to this process.

Overlay delivery uses a bounded intake-only bridge method with a pinned matching
root `reward_policy.owner`; its monetary ceilings may remain zero. The only
appended core type is `cyberherd_message`, with an immutable idempotency receipt.
Payment/feed/reset type aliases and weather/information categories are rejected.
It cannot mint credit, confirm food or enter core's Nostr outbox. Exact ACK loss
retries return the same WebSocket event sequence. Membership threads wait for
confirmed unpaused status; canary permits overlay-only staging. Original artwork,
base payment/feed messaging and the prominent paused banner remain unchanged.

These queues do not claim to solve whole-stack snapshot rollback. Preserve
consistent journals, signatures and publication receipts during explicit recovery.
Financial execution claims continue to fence payout-only rollback separately.

## Remaining full-port work

Feeder announcements still need the frozen herd summary and one authenticated
active-stream copy without duplicating their existing regular note. Optional
invitation/pause/resume/evening lifecycle notices, roster/OBS cards and authenticated
operator/client interfaces remain separate work. Installed NIP-46/relay/network
qualification, wider recovery/patched-engine installation and explicit activation are
still required. Synthetic relay/signing fixtures do not prove household or public
relay acceptance. Record exact verification results only after the checks finish.

The [member delivery checkpoint](../testing/cyberherd-member-delivery-checkpoint-20261110.md)
records the completed 439-test suite, eight CLI checks, inactive archive evidence
and unchanged open dependency advisory.


## Optional once-per-cycle repost suggestion

A no-treat message may append the existing editable `repost_hint` template.
Selection is deterministic and occasional (one third of message identities), so
retrying cannot change wording. Both the frozen payout breakdown and current
active membership must lack an earned repost award, and the cycle must still
match. A later repost therefore suppresses a stale suggestion. Banned/inactive
members receive no hint. Template/category opt-out claims no unused slot.

An immutable unique `(owner, cycle, member)` marker commits in the same
transaction as the frozen public/human message, destinations and delivery cursor.
A failed insert rolls the claim back. Reopen, recovery and `msg:` source aliases
cannot add another hint or alter the original bytes. A new cycle can claim a new
slot. The hint is presentation only; it grants no points, payment or membership.
