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

## Qualified source and live repair

[PR #115](https://github.com/lightning-goats/lightning-goats/pull/115) is merged
and the corrected membership worker is installed. Reviewed source
`1146b1134862282ba91117fb06270a61d15bf5f3`, tested PR merge
`ab827de1b74764f423410d103cbcc2540eaa94c1`, and merged main
`a36564324157ba6daf3876be12d65b8316bced74` have the identical tree
`e23f27eef3928afd50e8d19288d9f5773563571c`.

All ten workflows passed, including strict Clippy, locked full-feature tests
(422 library tests passed, six ignored, plus integrations and required root
checks), dependency audit, real NIP-46 qualification, package and installed
systemd rehearsals. Rust run: `43611588259`; security: `43611588343`;
deployment: `43611588515`, systemd job `132499802860`. The final regression
also respects core read admission: redundant safety checks were removed and
additional fallback attempts are paced by 500 ms, with a fresh check before
publication. Safety rejection retains the exact intent for retry.

The trusted review artifact ZIP digest was
`941b25e2039584be355025a59f406b2a9a1f1d1264ea94f56ec638cd37a9daea`;
its fully verified review archive digest was
`448ba364f4fd0ff84033da0d1f45d3853bd609f36495188c76a96226d8198b31`.
The original runtime UID accepted the selected binary and unchanged live
configuration with isolated networking and no credentials or journal access.
Only `/usr/local/bin/cyberherdd` was replaced, with SHA256
`59188c6ace77ae8d09eafe6b37e7f66c7ebc70ba9ecb1a4a0d97e52c4c6ef969`.

The approved four-service repair stopped and masked all writers and took a fresh
native four-journal backup. Actual journal images were compared with that backup
and guarded unchanged through replacement and before resumption. A follow-up
metadata guard initially compared the staging SELinux `var_lib_t` label against
the correctly preserved installed `bin_t` label and halted while writers were
still stopped. Continuation verified the exact transaction's original metadata
and new digest, retained `bin_t`, and completed without restoring any database.
All configuration, credentials, other programs, financial limits, boot enablement
and feeder controls were preserved. All four services resumed under their original
non-admin identities, passed health checks, and subsequently showed zero restarts
and zero main-process exit status.

Protected review directory:
`/var/lib/lightning-goats-upgrades/cyberherd-relay-fallback-1146b11-review`.
Evidence SHA256 values:

- PREPARED: `b3b8ec7663a80031c13b75ff881380a0d89e0a1ecada1d0deac590c4f93d252e`
- APPLIED: `ff64da33d5dae25bf80fc130ec019bea7c723a33d00d593307c7b1dd0823ec6f`
- RESUMED: `cc1943ac19f7b34208f36bcc319b2c667e7912b81b37188e25def60af7597107`
- DELIVERY-VERIFIED: `153bed46d28b9ab8e4dda6dae9a17f22cbfa076d37f9f57c3f0362bcd8baa4ca`

## Actual automatic publication verified

The repaired running worker created native matching relay receipts. A later
read-only observation showed 12 member receipts and two lifecycle receipts,
compared with zero before the repair. Unlike the initial transport diagnostic,
the following events were published by the normal worker and independently
retrieved from Artx using pinned `nak req`, with signature verification enabled,
a cleared environment and an empty credential-free configuration. Their author,
kind, target and canonical ID were checked; complete public events are retained
in the protected delivery evidence.

- Today's tagged-note reply: `62524edb446839fab965cca1b18ed4807a0920dabd1e12094f23fe27ddcb7d1b`
  (kind 1), welcoming JP, with root/reply references to today's
  `b8ae94995f09565d128ec658184f9c1b9d24eab07e160e45c9321b5534b695d1`.
- Current-stream member welcome: `c6ceb6ec1625b326611ef8e3022748267a41a5ac5fb326e5fff27f296b5ef03a`
  (kind 1311), thanking Sat for the repost.
- Current-stream lifecycle invitation: `0aec87c51f002f338dae6d346aebb79c354dc42f79df802d46dbba3da8735070`
  (kind 1311), also acknowledged by Primal.

Both chat events reference the authenticated stable stream coordinate
`30311:cf45a6ba1363ad7ed213a078e710d24115ae721c9b47bd1ebf4458eaefb4c2a5:b7806f5f-788e-4f6d-96e7-5cf16ffb55e7`.
Unavailable relays still retry exact stored signed bytes; complete delivery to
every configured relay is not claimed. Existing historical queued messages keep
their original times, text and thread anchors. No receipt was injected and no
new test payment or physical feeding was submitted.

Post-repair feeder status remained 221 sats, zero due feedings, no unresolved
attempt and override OFF. The roster remains limited to three users; subsequent
normal discovery also awarded Sat's like, reaching four points (revision 19).
Public messaging is now live; the broader twelve-part CyberHerd acceptance and
actual provider payout/recovery acceptance remain separately tracked.

## Subsequent audience policy

After this rollout, the operator requested no CyberHerd messages in stream chat.
The [subsequent deployed opt-out](cyberherd-no-stream-chat-20261114.md) disables
future chat delivery, including retained retries, while preserving notes and
OBS/WebSocket messages. The chat events above remain historical evidence.
