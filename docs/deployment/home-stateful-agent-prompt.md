# HOME agent prompt — deploy the stateful Goats candidate

Work in `lightning-goats/lightning-goats` on the current agreed source. Read
`AGENTS.md`, the [HOME cutover plan](home-stateful-cutover.md), the
[existing HOME gateway handoff](home-gateway-agent-handoff.md), and the
[Cashu recovery inventory](cashu-recovery-inventory.md). Own the HOME candidate,
private ingress, runtime identities, and state proof. Coordinate the tested route
and journal contracts with the [VPS agent](vps-edge-agent-prompt.md).

Use [GitHub issue #184](https://github.com/lightning-goats/lightning-goats/issues/184)
for migration coordination. Check it on startup, before and after shared work,
at least every five minutes while waiting on the VPS agent or during cutover,
and immediately before assuming HOME write authority. Post verified readiness,
blockers and handoffs with your revision, redacted evidence, and next action
and owner. Explicitly acknowledge the VPS agent's handoffs in the issue.

1. Inventory the existing mint, reserve, feeder gateway, OpenHAB owner,
   price/weather source, WireGuard reachability, architecture and installed
   dependencies. Keep those existing service paths working. Verify capacity
   for the protected state, staged checkpoint, ciphertext, and rollback copies.
2. Prepare a dedicated HOME ingress limited to the authorized VPS WireGuard
   peer. Agree on its routes, ports, health signals and headers with the VPS
   agent. Keep the wallet, collector, signer, database, and OpenHAB interfaces
   private. Prove ingress is denied from an unauthorized peer and that operator
   recovery access remains available.
3. Prepare repeatable installs for the receiver, wallet, collector, signer,
   CyberHerd and delivery/reclaim/publication roles the VPS actually runs.
   Preserve service identity and peer checks, egress boundaries, protected
   configuration, and the single feeder owner. Stage the authoritative display
   cache from existing HOME OpenHAB and publish its narrowly scoped read route
   for the edge. Document how HOME-local clients reach the existing mint and
   collector origins while preserving the agreed authentications and keysets.
4. Exercise synthetic checkout, Cashu import, signed-event replay, cycle
   refresh, physical reconciliation, and required internal sockets using
   isolated data and no physical actuation. Rehearse decryption and restore of a
   privately accepted checkpoint with the exact schema verifier. Verify
   supplemental state outside the standard collector and the operator-owned
   off-HOME recovery copy. Keep writers fenced on any paid-state copy.
5. Post the redacted route-contract and destination readiness in issue #184,
   with the checks needed before admitting traffic. Exchange protected details
   through the approved private channel. During the operator-approved handoff,
   acknowledge the VPS writer-fence and checkpoint update and verify the fresh
   checkpoint plus supplemental stores. Confirm the VPS writer fence before
   assuming write authority. Reconcile deposits, invoices, payout work and
   pending physical UUIDs with their originals.
6. Post verified HOME health and sole-writer evidence in issue #184 before
   handing off the upstream switch. Observe an approved live contribution and
   feeding with the operator. If recovery is required after HOME writes, own the new fenced
   HOME checkpoint and reconcile it before any proposed move back.

Initial handoff deliverables are checked-in configuration examples or staging
commands, a tested ingress route contract, an exact runtime and state inventory
in protected storage, synthetic test evidence, and the HOME rollback procedure.
Report source/artifact revision, scope, actual result, unverified items and
the next dependency. Production activation, real payments and physical feeding
are operator decisions. Keep credentials and private inventory out of commits
and messages.
