# Paused feeding, durable credit and message context

Operator decision, 2026-11-09: payments and normal messaging continue while the
feeder is paused; funded feedings wait for unpause, and debit occurs only for an
actual confirmed feeding. The existing physical/financial worker already follows
this rule. The update makes queued funding explicit in messages and the overlay.

## Credit, queue and debit

The queue is the durable unconsumed credit pool, not a separate set of actuator
commands. At a 1000-sat threshold, 2340 sats funds two feedings and leaves 340
sats toward another. Override ON prevents new dispatch; those 2340 sats remain
unchanged across pause/restart. When unpaused, the existing serialized worker
rechecks the live gateway's override, remote-enable, caps and interval, reserves
a UUID, and asks the single trusted owner to feed. Only a correlated terminal
confirmation causes one 1000-sat debit and `feeder_confirmed` event. After two
confirmed feedings, 340 sats remains. Unpausing by itself never debits credit.

A feeding already in flight can complete after the switch is paused. That
confirmed physical feeding must still debit once. Its retained intent/unknown
UUID is not advertised as a fresh queued feeding. Pending confirmation reduces
the additional queued count by one, and messages explicitly state that further
feeding also waits for the original result. Ambiguity never authorizes a fresh
UUID retry. This update does not alter actuation, debit or reconciliation rules.

## User-visible behavior

- The prominent paused banner includes the number of queued feedings and states
  that they wait for feeding to resume.
- The credit/progress display stays tied to ordered durable financial state.
  HTTP availability responses cannot overwrite credit. The top line says
  "queued" rather than claiming a paused feeder is ready to actuate.
- Normal eligible Nostr payment/credit announcements retain their amount/goat
  content, then explain the pause and queue once in the progress text. Example:
  "Thank you for 2100 sats! Feeding is paused. We have 2 feedings lined up for
  when feeding resumes. Another 221 sats are saved toward the next feeding."
- Overlay scrolling messages omit the pause notice; the prominent paused/queued banner
  supplies that status. Information/weather stay overlay-only while paused.
- When no feeding is funded yet, the message explains that sats are saved and
  gives the remaining amount, without announcing an empty queue. Unknown/stale status is described as unavailable, rather
  than promising readiness. In-flight confirmation is distinguished from queue.
- Periodic informational and weather messages now continue while paused, using
  the same schedule and audience policy. The earlier legacy override-based
  suppression is superseded by this operator decision. Both remain overlay-only.
- The per-event 10-sat payment-announcement minimum is unchanged. Smaller amounts
  still update credit/queue progress without producing payment announcements.
  No feeder effect runs merely because credit crosses the funding threshold.
- Public Nostr remains disabled in the current canary stage. Once the isolated
  signer/publication stage is enabled, the same renderer includes the event's
  frozen pause/queue context in eligible payment/feed posts. Weather/information
  never gain Nostr eligibility from having a pause note.

The static cache versions for JavaScript/CSS are bumped to 5; refresh existing OBS
browser sources to load the expanded banner.

## Frozen presentation context and failure handling

Migration 0013 adds one advisory `feeding_presentation` row. A supervised,
best-effort worker reads only the existing narrow gateway safety route every two
seconds and updates mode/pause reason, threshold and observation time. It has no
physical-command method. The real feeder worker/gateway retain their separate
fresh safety checks; cached display readiness is never dispatch authority.

When a normal payment, operator grant, confirmed feed, information or weather
event is appended, the transaction snapshots this advisory state and the
financially consistent credit/pending count into the event's `feeding` object.
This context includes `paused`, `reason`, `queued_feedings`,
`awaiting_confirmation`, `observed_at` and `threshold_sats`. Cache observations
older than 15 seconds or in the future become unavailable/paused. Metadata read
failures omit enrichment and do not roll back provider settlement or local credit.
The original financial event still commits atomically with its credit/debit.
Legacy events with no context retain their existing presentation contract.

Message text reflects the state at event creation; changing the local switch
later does not rewrite a receipt or produce a different message on signing retry.
The live banner independently tracks current availability and current socket
credit. Saved Nostr outbox bytes retain the existing exact-retry behavior.
Queue messages carry no claim that queued actuation has already happened.

## Verification

Isolated regressions cover: paused 2340 credit retained with two queued feedings;
restart and safe grant retry; ready display metadata unable to bypass the actual
override; normal drain to 340 through harmless correlated confirmations; an
in-flight confirmation debited once after pause; stale/future advisory cache;
financial credit surviving missing display metadata; normal information/weather
while paused with no Nostr audience; frozen Nostr payment context; and browser
queue counts before/after unpause and confirmation without fabricated debits.

Production credit must remain below the operator-reserved 1000-sat feeding test.
Installation verification uses a private ledger-copy migration rehearsal and
read-only live state/overlay observations; it does not require a threshold-crossing
credit grant or physical actuation by the deployment agent.

Selected-source verification passed: formatting, strict locked Clippy, 256 Rust
tests with the dedicated real-nak namespace case explicitly ignored, all 20
mocked browser tests, and 126 deployment cases with five existing skips. The
security gate passed with the documented unreachable-RSA exception. The live
physical threshold test remains reserved for the operator.

## Installed acceptance — 2026-11-09

Source `1e333706eb29948df0dd3929daaa5b6c068e0dcb` was packaged with archive SHA256
`08ab7bf9aaebe3194099055f5d1a5dbd9f933fc9604d29d39d6f3ca561b95488`.
Before installation, a root-private, network-isolated copy of the actual schema-12
ledger migrated to 13 with identical financial/grant/feed/outbox/cursor row
fingerprints and 97 sats retained. No production credit or gateway command was
created by the rehearsal.

The native upgrade retained backup
`/var/backups/lightning-goats/receive-upgrade-20261109T010146Z`; the static deployment
retained `/var/backups/lightning-goats/obs-20261109T010250Z`. The root install
records identify the exact source/archive/binaries. Mode remained canary, Nostr
publication stayed disabled, and the operator's override remained under their
control. Live readback established schema 13 and a fresh override-paused advisory
row, 97 sats, three provider settlements, five existing operator grants, zero
feed attempts/debits and no Nostr outbox rows.

Actual public Chromium verification showed the prominent paused label and
"0 feedings queued · credit saved toward the next feeding." A separate read-only
WebSocket observer validated a genuine periodic weather event generated while
paused, with frozen `feeding.paused=true`, queued count zero and the normal
message prefixed by the pause/queue notice. No JavaScript errors occurred.
Existing history was observed without replaying its animations to viewers.
Production credit remained below 1000; no additional credit, threshold crossing,
physical actuation or home switch write was used for this verification.

## Queue-message wording follow-up

The operator requested removal of the user-facing safety-limits qualifier.
Queued-feed captions and the overlay banner now simply say they run after
unpausing/when feeding resumes. Physical safety enforcement, spacing and caps
remain unchanged. The script cache version is 6.

Installed source `093645dd431672cda0e5b10f9f8d835c4cc4d14d`, archive SHA256
`0d340e553522b312e69cef1d622d3651baf2d10b5d6c03cafe49d44d2d3e1f9d`.
Formatting, strict Clippy, 256 Rust tests (one existing ignored case), 14 overlay
browser tests, the documented security gate and archive smoke/checksums passed.
Native backup: `/var/backups/lightning-goats/receive-upgrade-20261109T012723Z`;
static backup: `/var/backups/lightning-goats/obs-20261109T012738Z`.
Public Chromium showed "1 feeding queued to run when feeding resumes." with no
JavaScript errors; a separate read-only replay verified eight existing messages
with the shorter native queue caption. Credit remained 1142 sats (one queued
feeding and 142 remainder), with no feed attempts or debits. Mode remained canary,
with Nostr disabled. No credits, payments or gateway commands were created.
