# Feed intent contention — source correction, 2026-11-11

The full native suite for source `1240974d798e74503cd20cca6d65dc9e1998875f`
failed in `reservations_serialize_one_owner_feed_budget_and_global_rolling_limits`:
`begin_feed_attempt` returned native SQLite error 5, `database is locked`.
CI run `42958315132`, job `130318600903`, records the failure. The separate
installed operator/restart rehearsal passed, but that is not a full source pass.

Intent creation used a deferred transaction for its pending-attempt, cooldown and
credit reads followed by insertion. SQLite may reject that read-to-write upgrade
when another connection is writing; waiting at the initial write transaction
boundary avoids the stale read snapshot. See
[SQLite transactions](https://www.sqlite.org/lang_transaction.html) and
[snapshot isolation](https://www.sqlite.org/isolation.html).

The correction acquires `BEGIN IMMEDIATE` before any intent decision. It retains
the existing busy timeout, pending/unknown-attempt refusal, credit/cooldown checks,
UUID authority and confirmation-only debit. It adds no retry that creates a fresh
physical request, schema change, funding authority or adjustment to paid history.

A new native file-backed WAL regression holds another writer while the account
has sufficient credit. An explicit deferred-read control must produce the native
busy error. Intent creation must stay pending until the writer commits, then
create one intent without consuming credit. Duplicate intent/confirmation refusal,
one confirmed debit and one event remain mandatory. Pool connections are warmed
first so connection creation cannot substitute for the transaction boundary.

Exact CI qualification and separately reviewed same-schema deployment remain
pending. Production has not been changed by this source patch; the operator's
paused feeder and current paid state remain preserved. Do not represent the
earlier operator fixture or a rerun of unchanged source as resolving this failure.
