# CyberHerd membership audit — 2026-11-16

Status: source correction under qualification; not installed. Cashu work remains
paused and is not included. This extends the pending
[optional-social liveness correction](cyberherd-intake-liveness-20261115.md).

## Morning evidence

Read-only live inspection finds one active member, nostrpoorgirl, in cycle 7.
The repost at 14:45:32 UTC was applied and produced domain event 36 plus a
durable welcome message. That new message has an unsigned intent and no saved
signature or relay acknowledgement. Existing historical messages and signed
events remain retained; their receipts do not prove today's delivery.

Jr's like at 14:55:02 UTC and repost at 14:55:04 UTC reference the same current
project note. They remain ordered inputs 74 and 75. The like was repeatedly
retried without an available profile, holding the later repost. Neither input
was lost. Both reposts have nonempty embedded content; empty kind-6 content is
also supported when its references authenticate the target.

Credential-free, signature-verifying public reads found Jr's signed kind-0
profile on their advertised public relays, with a Lightning address present.
The configured read relays returned their signed kind-10002 list but no profile.
The worker queried only configured relays for profiles and never followed that
list. Retrying the same lookup could therefore never admit this eligible actor.
No signed profile or membership result was injected into the live database.

Membership, payout and message workers also repeatedly report held operations;
membership core calls fail with broken pipe or connection reset. The core bridge
uses one shared two-per-second quota for two separately authenticated roles.
Membership shares a one-per-second client; payout's intake and execution loops
used separate, unpaced clients. Their synchronized retries can consume the
shared quota before membership event and publication safety reads arrive.
Five nonmutating safety requests under the actual member UID produced two
authenticated `ready`/Nostr-enabled responses and three connection failures,
separating admission failures from an intentional operator pause.

A separate non-signing check under the member UID found a signer transport
blocker: its configured three-relay bunker connection timed out at 22 seconds,
whereas Primal alone returned the expected project public key in 6.12 seconds.
The existing encrypted client credential stayed in memory; no project message
was signed, published or inserted into a journal. Only the existing client's
encrypted NIP-46 public-key control exchange ran. The payout signer's earlier Primal-only
configuration repair had not changed membership's separate connection list.

## Corrections and preserved contracts

- Resolve missing profiles through authenticated NIP-65 write-relay discovery,
  bounded to one supplementary query and eight public destinations. Reuse this
  in reward-profile refresh. Keep native signatures, actor matching, configured
  completion quorum, saturation handling and signed address removal authoritative.
- Reserve one core request per second and four connection slots per role, with
  independent four-request bursts. Total sustained admission remains two per
  second, total connections eight and database concurrency one. Payout retry
  traffic cannot consume membership's admission or connection slots.
- Share one paced core client across payout intake/execution loops, including
  clones used for reconciliation. Preserve fractional quota-refill time.
- Include operator requests in membership's slot clock with next-slot priority,
  preserving FIFO for background work and removing priority on cancellation.
  Individual core checks have a two-second deadline within the unchanged
  five-second HTTP handler bound. The installed rehearsal caught that bypassing
  pacing caused existing operator activation/amount commands to collide with
  background reads after their burst was exhausted; priority scheduling fixes
  the actual runtime instead of relaxing the rehearsal's assertions.
- Catch up one bounded authenticated core page during operator ordering checks,
  using normal atomic intake. Presentation traffic no longer keeps edits racing
  a moving background cursor. Observe ordering after the paced safety read and
  recheck that safety's freshness immediately before mutation. Confirmed feeding
  inputs are staged and still block edits until their original snapshot is frozen.
- For frozen protocol events, try individual configured bunker relays instead
  of requiring a combined relay connection. Reuse exact unsigned fields, keep
  the full round and child cleanup within the existing 45-second budget, prefer
  the last verified working relay across clones and rotate the starting relay
  after a failed round. Transport failure can fall back; malformed, changed or
  invalid signer output still fails validation without a bypass. No new signer
  key, authorization or relay configuration is introduced.
- Retain the earlier optional-social parking, malformed-input disposition and
  exact-identity recovery corrections so missing profiles do not hold later
  payments, feeds or unrelated interactions.

Core polling/authentication and financial ordering still fail closed. A bridge
outage does not authorize membership changes from stale feeder safety. No paid
input, feeding snapshot, payout reservation, signed message or replay identity
is replaced. Stream-chat and lifecycle opt-outs remain effective.

## Verification

New regressions cover two distinct members admitted from repost/like activity,
profile discovery on a signed outbox, one like and one repost award per cycle,
restart/replay idempotency and three durable public intents. Missing configured
quorum, failed or saturated fallback and newer signed address removal are tested
independently. Read-only, broadcaster and private relay tags never enter fallback.
The two-member regression also exercises private safety IPC, signing and three
matching synthetic relay acknowledgements. Signer tests cover one-relay fallback,
unchanged fields across clones, complete outage/recovery, credential redaction
and stalled-child cleanup. Quota regressions exercise payout flooding,
independent membership admission, fractional refill and shared-client pacing.
Six consecutive operator reads alongside queued background actors and cancelled
priority waiters also have explicit regressions.
A 130-event presentation tail requires two bounded observations, then permits an
authenticated settings edit. A subsequent paid input and confirmed feed retain
their ordered queue entries and conclusively refuse another edit. Real HTTP
coverage also verifies a command can catch up presentation-only traffic.
Existing suites cover malformed and
empty reposts, closed cycles, social retry/backoff, ordered payments and feeds,
frozen reward snapshots, signing/ACK retries and audience opt-outs.

The first expanded source passes strict Clippy, all five new regressions, the
advisory audit and isolated real NIP-46 acceptance. The full suite exposed two
older delivery/payout fixtures using unpaced clients; they now share the same
one-second pacing as their production counterparts, preserving all assertions.
Final test results and exact source identity will be recorded after qualification.
Production programs, configurations, journals and service state remain unchanged.
