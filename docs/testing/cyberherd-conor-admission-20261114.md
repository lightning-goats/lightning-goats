# Ordered admission progress — 2026-11-14

Conor's current repost `9d3b30ec543a52be7dab76616ef48de78fe03e55c72d5b5d7ecde89c6871aa38`
was observed and staged against today's project note. It remained at ingress 57
behind a like at ingress 55 from an account whose profile was unavailable; his
like was at ingress 56. No current-cycle admission decision had rejected Conor.
The three-seat roster consisted of zero-zap social members, so a verified repost
could displace a member under the existing rule.

Two progress defects were identified. Independent presentation workers consumed
the core's global bounded connection allowance and caused intake broken pipes.
A profileless nonmember like also waited indefinitely even though a like cannot
enter an already full herd, regardless of the missing metadata.

The membership runtime now shares one paced FIFO client across intake, view,
reward/result, overlay and publishing actors. The server's admission, UID,
capability, database and financial limits remain unchanged. Fresh feeder state
and target/day projection are checked after lifecycle I/O and pacing waits.

An atomic capacity-only path can decline a nonmember like without a profile,
only for a confirmed-current target with feeding ready and capacity still full
inside the same write transaction. It cannot grant a seat, invent a payout
address, bypass pause, or treat missing metadata as a negative payout address.
If any condition changes it retains the profile wait. Reposts and eligible
open-seat interactions still require authenticated profile data. Existing
receipts remain frozen and ordered input is completed through the normal worker.

Regressions cover six concurrent paced clients against actual server limits,
profileless full-herd likes followed by an admissible repost, pause preservation,
missing-profile repost deferral and open-seat deferral. Source qualification and
live application are pending; no membership has been manually inserted.
