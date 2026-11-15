# Public recipient relay routing — 2026-11-15 candidate

Operator decisions: supplement direct public publishing with Send It; publish
user interactions to their relays; fully use Nostr relay-list conventions; do not
measure downstream visibility. No read-after-publication probes, propagation
scores or event-presence measurement are introduced. Relay-list metadata queries
and matching publication ACKs are protocol work, not downstream measurement.

## Audience and lists

NIP-65 kind 10002 `r` tags with `read` or no marker identify recipient inboxes for
public mentions; `write`-only destinations do not. Only the named person's valid
BIP-340 signed record is accepted. Newer created-at wins; equal time uses lowest
ID per NIP-01. Future/expired/invalid lists and non-public/insecure destinations
are refused. Explicit empty/new write-only records supersede earlier inboxes;
refreshes cannot roll an observed head backward. Metadata is cached for fifteen
minutes (negative results one minute), with bounded users/relays/cache size.
NIP-19 npub/nprofile references identify mentioned people. Public nprofile and
p-tag relay hints are bounded metadata-discovery sources; only a verified signed
NIP-65 record authorizes extra public delivery destinations. Hints never become
private inbox authority. Discovery uses configured public
readable relays and excludes Send It, which denies subscriptions.

New CyberHerd notes freeze base plus recipient read relays with their immutable
message record, outside network work/write locks and with source-cursor recheck.
Their existing per-relay outbox/receipts retry exact signed bytes. Core public
notes likewise freeze a per-event route plan and retry each destination fairly,
with at most four network attempts in flight. Direct configured relays precede
recipient additions and metadata work follows note work. Up to eight addressed
users/eight inbox relays each fit the 72-destination public limit.
Outages do not change financial accounting, feeder work or signed text.

The author's existing signed kind-10002 list is preserved when configured storage
write relays are added; existing read policy is not replaced. Missing author
metadata never authorizes inventing/replacing a list. New advertisement fields
are frozen before NIP-46 signing. Core copies the retained author list to public
note destinations through durable metadata route extensions. CyberHerd attaches
its observed author list best-effort on successful member-note destinations.
The write-only broadcaster is not advertised as a readable author outbox.

NIP-17 gift wraps use the separate kind-10050 inbox list, as the existing browser
contact flow already does. Public NIP-65 read lists and broadcasters never become
private-contact fallbacks. Additional browser/backend/retained-delivery guards
reject Send It for private contacts. NIP-57 receipts continue to honor the exact
signed zap request's receipt relays, independently of membership admission.

## Durable state without financial/schema migration

Core publication uses the existing extensible event-log contract for immutable
`nostr_publication_plan`, `nostr_publication_plan_extension` (author metadata only),
`nostr_publication_attempt`, `nostr_publication_ack`, `nostr_relay_metadata_intent`
and `nostr_relay_metadata_observed` records. The original signed outbox is retained.
Leased attempts and exponential per-relay retry prevent a failed destination from
blocking later notes; accepted destinations survive restarts. Broadcaster acceptance
is recorded separately and cannot alone mark an event directly delivered. A relay
ACK is not evidence of wider propagation or retention guarantees. No old published
payment notes or suppressed lifecycle notices are bulk replayed.

These event types never render as public payment/feed messages or create paid
credit. All four existing journals and replay history remain authoritative; no
new journal, migration, credential or spending capability is added.

## Current status

The Send It addition is live in core/CyberHerd public publishing configuration,
with existing direct relays, contact/bunker routes and lifecycle/chat opt-outs
preserved. Root-private review:
`/var/lib/lightning-goats-upgrades/sendit-broadcast-20261115-review/RESULT.json`,
SHA256 `51c9f76957bfbc9d964273da8af9f36d05624eaeecc4f327fca025579f30d8ed`.
Only core/member services restarted; no program/schema change occurred. The initial
immediate readiness check raced startup; later health checks passed without another
restart. No downstream visibility query was performed for this rollout.

The code changes above are not yet deployed. Formatting/whitespace checks pass;
full Rust/audit/native CI and exact program-only deployment qualification remain
pending. Synthetic regressions cover NIP-65 direction, signed replacement/ties,
cache rollback/outage, NIP-19 identity, NIP-17 separation, frozen member routes,
core retry fairness/restart and immutable metadata signatures/routes. No real
payment or physical feeding test is submitted by these regressions.

References: [NIP-65](https://github.com/nostr-protocol/nips/blob/master/65.md),
[NIP-17](https://github.com/nostr-protocol/nips/blob/master/17.md),
[Send It](https://github.com/Spl0itable/nosflare#send-it-blaster).
