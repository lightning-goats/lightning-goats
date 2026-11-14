# Verified member-result handoff checkpoint — 2026-11-10

Feature code `d59f6983a7a57c1a561cb3ea93e4470a63e60f52` was tested as PR merge
`dffc2a4a931e0804ad8f9f22986fb20d541e0d72` into
`18e0261add12d78b6e567f471bb310ba8aec3898`. Both trees were independently
confirmed equal to `402fe67c1e408e9665ab8d632a53f591220db5d7`; the review archive
records the actual merge checkout.

[CI 42752319393](https://github.com/lightning-goats/lightning-goats/actions/runs/42752319393),
job `129618571949`, passed formatting, strict locked all-target/all-feature Clippy,
and `cargo test --locked --all-features`: **429 passed, zero failed, one ignored**.
The separate inactive preparation example passed two tests and its non-applying
artifact check; those two are excluded from 429.

Completed logs explicitly show all four result regressions passing: actual core
final verification and role separation; atomic frozen-breakdown receipt/event
commit across concurrent retries, pages and restart; changed member/net/gross and
manifest rejection; and the independent result worker waiting before verified
results exist. The existing five execution-claim/rollback regressions still pass.

[Deployment 42752319198](https://github.com/lightning-goats/lightning-goats/actions/runs/42752319198)
passed all five jobs. Package job `129618570910` built the binaries, passed seven
non-skipped CyberHerd CLI checks and validated the inactive review archive.
Browser, nginx, harmless owner, isolated package install and existing core systemd
fixtures also passed. This does not establish installation of the new roles.

[Real NIP-46 42752319352](https://github.com/lightning-goats/lightning-goats/actions/runs/42752319352)
passed the existing isolated regression. It does not qualify a new messaging
runtime or the outgoing broker against the household bunker.

[Security 42752318967](https://github.com/lightning-goats/lightning-goats/actions/runs/42752318967),
job `129618567631`, failed on unignored **RUSTSEC-2026-0459** in
`libsqlite3-sys 0.30.1`, reported fixed at `>=0.39.0`. RSA remains unreachable.
This patch changes no dependencies and does not waive the finding.

The [verified handoff](../architecture/cyberherd-verified-results.md) is now source
verified; a `payout_result` domain event is not a public treat announcement.
Note/chat/overlay outboxes, real joining receipts, editable outgoing zap text and
joining references, lifecycle and authenticated interfaces remain work. Wider
consistent restore/failed-cost tools, dependency remediation, installation
qualification and explicit live financial activation are also required by the
[full contract](../planning/cyberherd-implementation-plan.md). No production
service, payment, relay post, feeder or credential was changed by this checkpoint.
