# Three-user capacity — live checkpoint, 2026-11-11

The operator requested **3 CyberHerd user spots** and reuse of the existing
Lightning Goats Nostr messaging identity for operator access. Both are applied.
The member reward allocation remains **10%**; no member was removed.

Source `8e28380da6502bca647aeaabd35289b7b4e1d026` adds the authenticated,
configuration-only project-operator transaction and signed capacity client.
Its 27-file root tool closure was independently compared against committed bytes
and externally hashed before execution. The actual installed native parser
accepted the exact candidate as the original membership UID with no network or
capabilities. The reviewed application changed only membership configuration and
restarted only the membership service. All other original protected file hashes
and complete core/outgoing service states independently matched afterward.

The first signing attempt timed out while using the publication relay list;
the stored capacity remained 10. Correction
`9675a179d987324f67acc958a27aaeecf37117ed` uses the already verified household
Primal signing transport independently of the publication relay list, and emits
only sanitized signing diagnostics. The corrected client ran in a transient
non-admin systemd unit with its existing encrypted NIP-46 client credential.
It received the exact project-authored kind-27235 proofs, verified their hashes
and signatures, submitted one audited settings command and verified its receipt.
No project signing key was created or copied, and no note was published.

The signed before/after comparison preserved the minimum, reward percentage,
members and cycle. Independent native-loopback and certificate-verified public
HTTPS reads both returned capacity **3**, active members **0**, member percentage
**10**. Core payment/feeding process and outgoing services retained their original
states. No payment, feeding, payout, financial-limit change or Nostr note was
requested. The existing project messaging identity is the only operator in the
live allowlist. Public operator-page rollout remains separate.

Local format and diff checks passed. All **154** upgrade regressions passed in
disposable root-owned fixtures with networking disabled, including all ten new
operator/capacity cases. Source `8e28380` also passed the
[Rust gate](https://github.com/lightning-goats/lightning-goats/actions/runs/43035904941),
[security gate](https://github.com/lightning-goats/lightning-goats/actions/runs/43035905076)
and deployment root/package checks. Full deployment/systemd qualification of
the separate financial-maintenance implementation is tracked separately; this
checkpoint does not claim financial activation or recovery adoption.

The earlier sealed program-upgrade preparation predates this configuration and
member-process change. It is stale and must be prepared again before use.
