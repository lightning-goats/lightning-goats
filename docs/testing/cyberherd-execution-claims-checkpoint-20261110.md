# CyberHerd execution-claim checkpoint — 2026-11-10

Feature code `3c0cb3d4213cfb94fac3aef4df305e1e55def2fc` was tested as PR merge
`5adcdacd2de20404407089d1b123c7018325b01c` into
`18e0261add12d78b6e567f471bb310ba8aec3898`. Git tree comparison confirmed both
trees equal `f667557926782616033bcf3e73655678a5d93011`. The archive records the
tested merge SHA, not a different deployment provenance.

[CI 42751159985](https://github.com/lightning-goats/lightning-goats/actions/runs/42751159985),
job `129614844823`, passed format, strict locked all-target/all-feature Clippy,
and the full locked all-feature suite: **425 passed, zero failed, one ignored**.
The separate inactive example's two tests passed and are excluded from 425.

The log explicitly confirms both real SQLite snapshot rollback fixtures passed:
missing original quote history stops before a replacement invoice/quote/payment;
retained original quote history recovers the same payment after actual quote
expiry despite losing local execute/binding records. It also confirms immutable
claim retention, role/policy/public-authority rejection, and refusal of claimed
no-execute/omitted-member manifests. Existing disabled execution, uncertain
submission and independently verified terminal-accounting tests still pass.

[Deployment 42751160021](https://github.com/lightning-goats/lightning-goats/actions/runs/42751160021)
passed all five jobs. Package job `129614845045` built all binaries, passed seven
non-skipped CyberHerd CLI checks and validated the inactive three-binary archive.
Existing browser, nginx, harmless owner, isolated install and core systemd tests
also passed. These remain fixtures, not installation of the new CyberHerd roles.

[Security 42751160018](https://github.com/lightning-goats/lightning-goats/actions/runs/42751160018),
job `129614844655`, failed on the unchanged, unignored **RUSTSEC-2026-0459** in
`libsqlite3-sys 0.30.1` (fixed at `>=0.39.0`). The RSA reverse-graph guard passed.
No dependencies changed and this result does not waive the advisory.

The [claim protocol](../architecture/cyberherd-execution-claims.md) addresses
payout-only rollback while core retains newer history. Whole-stack consistent
restore, explicit failed-cost reconciliation, installed identity/network/helper
qualification and live financial activation remain open. Member-result delivery,
editable outgoing zap wording/joining references, durable conversation outboxes,
lifecycle and authenticated interfaces still belong to the full port. This
checkpoint authorizes no production installation, payout, Nostr post or feeding.
