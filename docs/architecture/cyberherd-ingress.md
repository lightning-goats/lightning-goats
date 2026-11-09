# CyberHerd authenticated ingress

These library components authenticate targets and social input, persist replay
state, and apply membership transitions. They do not start a CyberHerd daemon,
hold payment/signing/gateway credentials, or authorize financial or feeder work.
The [core bridge](cyberherd-core-bridge.md) and
[intake worker](cyberherd-worker.md) now connect these components in source.
The [complete implementation contract](../planning/cyberherd-implementation-plan.md)
still includes funded payouts, delivery, interfaces and operational qualification.

## Calendar and target authority

The operator-selected calendar is `America/Denver`, including daylight saving.
`DayWindow` calculates real local-midnight boundaries through pinned IANA timezone
data, including 23/25-hour days, midnight gaps and skipped dates. It does not use
process-global `TZ`, fixed 24-hour increments or rounded minute scanning.
`Store::catch_up_calendar` is an idempotent optional operation; scheduling it and
obtaining authenticated feeder availability remain worker responsibilities.

`TargetPolicy` identifies the project and configured announcement tags. A current
announcement must be a signed project kind-1 note within that local day, matching
a configured tag or the legacy content-hashtag fallback. Prefer a note linked to
the active stream's goal; otherwise choose the most recent tagged project note.
Announcement-candidate indexes are keyed by the normalized policy so changed tags
cannot silently reuse an old allowlist's candidates.

Kind-30311 stream authority requires one of:

- The configured project authored the signed event.
- Its NIP-53 host proof signs SHA256 of the exact stream coordinate.
- A fixed supported provider's HTTPS metadata authenticates the publisher for
  this project. An event-supplied arbitrary service URL or host tag is insufficient.

The provider adapter bounds and pins public TLS/DNS destinations, caches successful
responses, expires authority, and permits at most one metadata attempt per
provider/project/observation timestamp. This includes failed requests, so a burst
of signed forged publishers cannot multiply outage requests within one batch.
A later observation may refresh a mismatched key to accommodate rotation.

Persist authenticated stream versions and monotonic heads. The NIP-01 latest
timestamp/lowest-ID tie rule prevents an older live event from replacing a newer
ended head. Fresh live streams can cross midnight. Older authenticated versions
are aliases of the same active coordinate; merely supplying a raw signed event
with a matching coordinate does not grant an alias. Linked kind-9041 goals must
be signed by the project, have a positive amount/relay context, and remain open.

## Profiles

Profiles are native-verified kind-0 events for the interacting signer. NIP-05 is
ignored. Names are bounded and sanitized; invalid optional picture/address data
is dropped. A later signed profile removing its payout address supersedes an
older usable address, including after restart. Failed lookups are not evidence
that an address is absent.

## Relay reader

The reader opens only configured public WSS destinations through the pinned
public-network adapter. It holds no signer and sends only `REQ`/`CLOSE` plus
protocol pong/close frames. Every accepted event must have a valid canonical ID
and BIP-340 signature, match the subscription, and satisfy a requested filter.
Wrong-subscription, malformed, duplicate, unrelated and invalid-signature events
cannot become domain input.

Event-ID and address-coordinate engagement queries are separate OR filters.
Putting `#e` and `#a` in one filter would incorrectly require both. Filters,
configured relays, messages, response bytes, event counts and deadlines are
bounded. Results distinguish EOSE completion, failures and saturation. A saturated
or interrupted read cannot justify advancing a complete-backfill cursor. EOSE
alone is not cryptographic proof that an unknown event does not exist.

## Social parsing and durable recovery

`SocialEvent` accepts supported signed kinds 6/7/16 and excludes project self
interactions. Standard `+`/empty reactions count as likes; downvotes, emoji,
custom-emoji reactions and chat text do not. For NIP-25 the final `e` identifies
the actual target, so an earlier ancestor reference cannot qualify an unrelated
reaction. Repost bodies, when present, must contain the actual signed target;
IDs, kinds, author hints and coordinates must agree. A version-specific generic
repost without an `a` includes the full signed event. Kind 6 is for notes; generic
reposts support the selected stream and linked goal.

Persist supported raw input before profile/network lookup. The pending inbox has
a 4096-entry per-owner admission cap, bounded due batches and retry backoff;
completed identities are not reopened by replay or a later failed attempt.
The read projection has explicit count and byte budgets. Historical retention
and operational capacity monitoring remain part of the worker/install work.

The processing adapter uses `nostr:<canonical-event-ID>` as its source identity.
It freezes a verified profile in the first domain envelope and reuses that exact
envelope on deferred retries. A missing profile remains pending. Paused/unknown
feeder state or unavailable target discovery cannot claim membership as applied.
Recovery must refresh targets before processing the batch. Confirmed stale work
gets a minimal discard receipt without fetching an unnecessary profile.

Applied membership, source receipt, snapshot and authenticated conversation
context commit together in the separate CyberHerd store. A crash before marking
the raw inbox complete reuses the committed receipt and creates no second award
or domain event. The frozen context supplies original destination evidence to
the later note/chat outbox; it is not itself a publication or joining-message
anchor. Current stream copies and persistent per-member threads still require
the conversation/delivery worker.

## Verification scope

`tests/cyberherd_targets.rs` exercises actual separate databases across reopen,
signed negative profile replacement, ended-head replay, raw inbox retries,
Mountain-time catch-up, signature/reference consistency, stale/unavailable
targets, profile freezing and exact-once social application. Module tests cover
calendar transitions, host/provider authority, note/goal selection and an actual
WebSocket protocol exchange over a private in-memory transport. These do not
attest to household signing, public relay completeness, spending or physical food.
