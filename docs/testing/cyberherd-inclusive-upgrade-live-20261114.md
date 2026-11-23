# Live inclusive-fee schema maintenance — 2026-11-14

The operator explicitly approved maintenance after the separate staging approval.
The [qualified review](cyberherd-inclusive-upgrade-checkpoint-20261113.md) was
applied and resumed successfully. This completes the additive schema/program
upgrade, not financial activation or the full twelve-part CyberHerd port.

## Exact authority and retained state

Selected artifact source: `ae10df666741c9160ada3b54a160435dc36f2b25`.
The independently rechecked launcher, tool manifest and sealed review digests
matched their staged values before execution. `PREPARED.json` remained
`41a7c373e1dbc43754a1a3b30715383c47d0a0fd33a2d2308b1aebbf86bbb3c9`.
Core and membership were active; payout and signer were masked.

The authenticated `apply-inclusive-fees` operation completed with:

- A fresh accepted four-journal native backup.
- Successful runtime-UID clone migration and four-journal comparison.
- Replacement of the six selected programs with reviewed bytes.
- Successful offline live payout migration and fresh native comparison.
- Configuration unchanged, activation unauthorized and no services started by apply.

Accepted backup manifests independently show the exact payout migration-history
transition from seven records (`1,2,3,4,5,6,8`) to ten
(`1,2,3,4,5,6,8,9,10,11`). Original rows, signed bytes and the other journals were
preserved by the strict native comparisons. No stale database restore or original
incident-credit retirement was performed. Both `APPLIED.json` and `RESUMED.json`
exist in the protected review.

## Resumption and observed behavior

The separate authenticated `resume-inclusive-fees` operation completed its fences
and started only core and membership. Configuration remained unchanged and
`payouts_enabled` was false. Both outgoing units remained masked.

Post-resumption observations:

- Public status: active, 121 sats feed credit, zero feeds due, no unresolved feed
  attempt, gateway reachable and local feeder override ON.
- Primary herd LNURL metadata: `payRequest`, min 1,000 msat, max 1,000,000,000 msat,
  Nostr supported. No invoice callback or real payment was submitted for this check.
- All six installed executable hashes match the sealed selected program hashes.
- Existing credits were retained; no new feeder command, manual test credit,
  provider execution or payout activation was initiated by this maintenance.

This is live maintenance and readback evidence. It is not proof of real outgoing
transfers, complete external recovery adoption, all incoming-zap paths, public
CyberHerd conversation delivery or lifecycle behavior. Those remain open in the
[acceptance inventory](cyberherd-acceptance-matrix.md). Existing household safety
and override controls remain authoritative.
