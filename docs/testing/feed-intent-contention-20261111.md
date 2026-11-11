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

Corrected source `55ed77b8a3f0c7cb50b031536cb53161de6d5e89` passed all ten
workflows: CI `42960188544`, Deployment `42960189058`, Security `42960188767`
and real NIP-46 `42960188494`, plus the six network/home workflows. The Rust
job `130324615440` passed formatting, strict Clippy and the full native suite,
including the original failing case and the new contention control/regression.
Its library result is 376 passed, four separately exercised root-only cases
ignored in the ordinary pass; required privileged fixtures also passed.
The proposed merge `dd7c4a28ce799a1e9da797843eae16b5165a2017` has an identical
tree to the branch head, independently checked through GitHub comparison.

The subsequent [current-schema program update](current-schema-core-patch-20261111.md)
passes exact native/systemd qualification and actual protected VPS apply/resume.
The corrected core is now running. The operator's paused feeder, current paid
state and contact configuration remain preserved, with no real feeding/payment
used for acceptance. The earlier failing run is retained above.
