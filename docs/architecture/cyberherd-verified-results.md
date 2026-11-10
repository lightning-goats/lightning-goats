# Verified payout results returned to membership

Core exposes a bounded, intake-role-only read of final verified outcome rows.
The payout role cannot call it. Before final core verification it returns waiting,
even if the payout journal already says Completed. It performs no new provider
call and remains readable with execution disabled. Each page carries the exact
reservation/plan/epoch, manifest identity and up to sixteen sorted member results.
There are no provider payment IDs, invoice bytes/hashes, callback URLs, credentials
or transfer capabilities in the response.

The membership process accepts only the opaque result page returned after an
actual kernel peer check. It compares its immutable core binding, original
reserved plan, eligible payable rows, each gross/net allocation and fee ceiling.
Unknown members, changed amounts, skipped pages and changed manifests fail closed.
Pages, per-member `payout_result` events and monotone progress commit together.
Exact retries and restarts preserve the original events without creating duplicates.

Each private result event freezes the member's original payout breakdown from
the plan, including its contribution/points/share snapshot and roster revision.
Later zaps, membership changes or daily resets cannot change that queued result.
Failed and unsubmitted results retain distinct statuses; neither becomes a treat.
No carried member balance or financial mutation is introduced here.

An independent bounded result worker polls pending plans in round-robin order.
One unresolved earlier distribution cannot starve later verified results. It runs
with configured reward planning, independently of relay discovery, recipient
lookup, payout execution and future public delivery. Core outages leave the cursor
and original receipts available for retry.

This is the accounting-to-presentation handoff. It does not yet sign/publish
messages or invent joining anchors. Public treat delivery still needs the frozen
note/stream branches and real joining publication receipts required by the
[full implementation plan](../planning/cyberherd-implementation-plan.md). Those
receipts are separate from a recipient's optional kind-9735 zap receipt.

Temporary-database fixtures cover final core verification and role separation,
minimal responses, atomic failure/retry, changed allocation rejection, paged
restart recovery and a result worker waiting before verified data exists. Results
must be pinned to the exact tested source when verification finishes. No new
service is installed or enabled by this source.
