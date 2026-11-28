# VPS agent prompt — prepare the public Goats edge

Work in `lightning-goats/lightning-goats` on the current agreed source. Read
`AGENTS.md`, the [HOME cutover plan](home-stateful-cutover.md), the
[mint payment boundary cutover](mint-payment-boundary-cutover.md), and the
[coordinated Cashu collection procedure](cashu-backup-collection.md#coordinated-collection).
Own VPS traffic, static assets, source quiescence, backup, and the upstream switch.
Coordinate with the [HOME agent](home-stateful-agent-prompt.md).

Use [GitHub issue #184](https://github.com/lightning-goats/lightning-goats/issues/184)
for migration coordination. Check it on startup, before and after shared work,
at least every five minutes while waiting on the HOME agent or during cutover,
and immediately before switching the public upstreams. Post verified readiness,
blockers and handoffs with your revision, redacted evidence, and next action
and owner. Explicitly acknowledge the HOME agent's handoffs in the issue.

1. Inventory the running receiver, wallet, CyberHerd, collectors and delivery
   workers, protected configurations, durable stores, and their activation
   sockets, timers, helpers and recovery markers. Record the exact running
   versions and state boundaries. Separate the dated deployment evidence from
   assumptions in examples. Preserve unrelated VPS apps.
2. Build a public route map from the actual running Nginx configuration. Preserve
   DNS, TLS, static homepage and OBS, mint hostname and existing mint routing,
   canonical addresses, approved methods, WAF rules, signing headers, body
   limits, timeout and log handling. Use the agreed HOME private listener for
   dynamic Goats routes. Keep the deny boundary for other paths and suppress
   proxy retry of mutation requests after ambiguous responses.
3. Prepare isolated VPS test routing with the HOME agent. Verify the six
   Lightning Addresses, wallet request and status flows, checkout, WebSocket
   resume and clean snapshot, retired route denials, public TLS, and upstream
   failure from an actual browser and an independent network vantage point.
   Keep a static outage view available and suppress payment controls if HOME
   is unreachable. Smoke-test Nginx reload and rollback.
4. Script the maintenance sequence and rehearse it. For approved cutover, gate
   new public mutation admission, verify dispatch is held, reconcile original
   physical UUIDs, and fence all source writers and activators. Make a fresh
   coordinated Cashu checkpoint plus any supplemental stores. Verify checksums,
   scope and off-HOME recovery copy before sending the approved encrypted
   checkpoint to HOME. Post the verified admission hold, writer fence, and
   checkpoint handoff in issue #184. Preserve issued invoice and receipt
   identities and the original VPS programs, journals and Nginx configuration.
5. Acknowledge HOME's verified readiness in issue #184 and wait for the operator's
   traffic-switch decision. Verify the VPS fence again, validate the selected
   configuration with `nginx -t`, and switch only the approved upstreams. Check route parity
   and signed/protected paths, restart/reconnect behavior and the public
   maintenance path. Correlate the operator's live payment, gateway result,
   debit, new cycle and viewer progress with HOME evidence.
6. Keep the VPS unable to write after cutover. A recovery after any HOME write
   requires a new accepted HOME checkpoint and original-operation reconciliation
   before the VPS can resume. Publish the redacted cutover record and retained
   rollback locations. Decommission or resize separately after operator review.

Initial handoff deliverables are a route matrix, an isolated edge rehearsal,
a named list of writer fences and recovery stores, the tested Nginx switch and
rollback, and an evidence template. Report exact revision, scope, results,
unverified items and the next dependency. Keep secrets, Cashu tokens, customer
data and private inventory in protected storage. Live admission, payment,
physical operation and cutover remain operator decisions.
