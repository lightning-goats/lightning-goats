# Zap payment and feeding attribution — implementation in progress

The operator requested natural variations of payment and feeding messages that
identify verified zap senders with profile links and use the actual zap amount.
Ordinary Lightning payments retain their existing wording. The current upstream
CyberHerd explanation was fetched again on 2026-11-14; its SHA256 remains
`e56c6793ed0687e4fcac7df54dac48a375673308dc65e988e0dcd987af46e67a`.

The candidate derives attribution from authoritative settled payment records,
validates the exact signed NIP-57 request and invoice description hash, verifies
invoice amount and payment hash, and uses the signed author rather than comment
text. Anonymous requests use a generic anonymous label and no sender mention.
Distinct settlements retain distinct event identity even with a reused request.

Core Nostr and overlay paths use this attribution. The separate confirmed-feed
presenter matches the core's immutable announcement identity against the actual
consumed credit source, before the feeding's frozen ingress barrier. Matching
only an amount or selecting the latest payer is prohibited. Lookup is read-only
and paginated. No ledger migration or accounting mutation is introduced.

The existing payment minimum, ready-feeding payment suppression, confirmed-feed
requirement, one announcement per payment, operator template opt-out and frozen
signed retries remain required. CyberHerd stream-chat remains disabled.

This is not deployment acceptance. Formatting passes locally. Exact-source Rust,
security, integration and native deployment checks have not yet completed.
Further work must cover exact consumed-credit fixtures, persisted template
seeding/deletion behavior, anonymous and public signing tags, and installed
acceptance before replacing either production program. No test payment, feeder
actuation, journal edit, service restart or public note was performed here.
