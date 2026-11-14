# Repost discovery repair — 2026-11-14

The operator reported a repost of today's CyberHerd note without membership
admission. The services were running and the feeder was ready, but the public
roster exposed no current target. Discovery queried only Damus, Chaima and Artx;
its read list omitted nos.lol and Primal, which the announcement/engagement
publishers also use. A signed repost was visible on Artx while its original note
was absent from that relay's result, so normal target-based recovery had no
authenticated current announcement to attach the repost to.

Unauthenticated public reads retrieved and verified today's announcement
`b8ae94995f09565d128ec658184f9c1b9d24eab07e160e45c9321b5534b695d1`.
Its canonical ID was independently recomputed, the author matched the configured
project and its creation fell on today's America/Denver day. The observed repost
`a587d52d25962d473b32f06c082abe1426ecc12d780d65f3baef13f946befe6d`
referenced that exact note and project. Relay timeouts/empty individual reads
remain distinct from proof that an event never existed.

## Approved repair and observed recovery

The staged candidate appended nos.lol and Primal to the top-level read-relay list,
preserving the previous three relays and every other parsed configuration field.
The actual membership binary accepted it under its original UID with isolated
networking, cleared groups/capabilities and no credentials/runtime/store access.
Preparation SHA256:
`fdac3f2841ea9e8f151f52e65c95860a1d41f55fa8ae96bdb436f3cd098a5ef6`.

Automatic approval review initially rejected the production configuration/restart
because diagnosis did not explicitly authorize that cutover. No application was
performed by the rejected action. The operator then explicitly approved the relay
repair and membership-only restart. The authenticated file primitive retained
the original configuration/metadata, applied the one-file change, and restarted
only membership. Other financial configurations/services and feeder controls were
unchanged. No journal was manually edited or membership injected.

Normal signed-event discovery/backfill subsequently exposed the correct current
announcement, linked goal and authenticated live stream. The public roster
advanced from revision 15 to 17 in cycle 5: Sat received the repost award of three
points; JP received one reaction point. Their displayed shares were 7.5% and 2.5%,
respectively, from the selected 10% allocation. Capacity remained three.
This is actual admission through the running service, not a synthetic grant or
an ordinary payment being treated as a zap.

The configuration example now includes both additional read relays to avoid
repeating the initial omission. It remains disabled with deliberate identity/
epoch placeholders. Discovery/read relays and public-message destinations remain
separate settings.

## Separate public messaging stage

The operator subsequently requested that CyberHerd messaging be finished and
published. The prepared stage enables the already authorized membership client
for kind-1/1311 delivery, lifecycle notices and the single delegated feeder
presenter. It preserves all payout limits, incoming payment authority, identities,
original delivery intents and signed history. Native four-role/original-UID checks
and assembled service-unit validation passed; preparation SHA256 is
`a77dd8fc535b7f79d9e4577cbda7a9947f5e2e4a731c36d8219c3bf0362008b7`.

The existing membership authentication credential's public identity was verified
without emitting secret material. A dedicated encrypted-client credential mapping
is required by the service; no new client or project secret is introduced.

Automatic approval review required explicit approval for interrupting all four
services. The operator approved that maintenance. The transaction then stopped
and masked the four roles, took and verified a fresh native four-journal backup,
installed the two configuration changes and existing encrypted-client mapping,
and resumed all four services. No journal image, financial limit, program,
feeder override or signed intent was replaced. Applied evidence SHA256:
`22031ce0f001910908c64d93e430618441cc0d098cee93555239db68b1154e7d`.

The current roster subsequently reached three members: Sat (three points),
Bitcoiner Girl and JP (one point each), with shares 6%, 2%, 2%.

## Publication failure found after activation

Activation alone did not establish delivery. Read-only presentation diagnostics
found saved signatures but zero native delivery receipts. An unchanged retained
public event was transmitted to its original three destinations for diagnosis:
Damus returned HTTP 521, Chaima returned HTTP 404, and Artx returned a matching
`OK` with `true`. That diagnostic acknowledgment was not injected into any journal.
The worker always chose the first missing relay and therefore never reached Artx.

The correction selects all eligible unacknowledged frozen relays and tries them
in order within a bounded 180-second publication attempt (at most eight relays, each with a bounded native transport). It retains the original
signed event, validates the actual acknowledging relay/event, and saves only its
real receipt. Membership safety is refreshed before each attempt; lifecycle
ordering remains per destination and relay. A 300-second lease covers bounded
signing (including the shared signer wait) and publication. No schema, monetary accounting, frozen destination list,
event timestamp or financial policy changes. Regression coverage exercises both
note/chat fallback and lifecycle reuse after a lost first-relay acknowledgment.

Source qualification and deployment of this correction remain pending at this
checkpoint; do not treat the diagnostic public event as proof that the automatic
outbox or complete CyberHerd port has passed acceptance.
