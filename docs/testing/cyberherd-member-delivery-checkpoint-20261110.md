# Member conversation and delivery checkpoint — 2026-11-10

Feature code `74fd69ed0773ab405928b4df9ce43c18198851c7` was tested as PR merge
`0b62aed42cd9d0c2f88d1686c895a05d2270951a` into
`18e0261add12d78b6e567f471bb310ba8aec3898`. Independent local/GitHub tree
comparison confirmed both equal `d69108a46cf180a6afce05d25889d1161f7eaa97`.
The review archive records the actual tested merge checkout.

[CI 42757896632](https://github.com/lightning-goats/lightning-goats/actions/runs/42757896632),
job `129636442652`, passed format, strict locked all-target/all-feature Clippy and
the full locked all-feature suite: **439 passed, zero failed, one ignored**.
The separate inactive cross-host preparation example passed two tests and its
non-applying artifact check; those two are excluded from 439. Direct GitHub run
and job status also confirmed terminal success after log availability caught up.

Completed logs explicitly show all ten new regressions passing: lost-ACK
note/chat restart recovery without re-signing; independent joining roots;
signature/ACK conflict rejection; pause/unknown/non-public holds; the payment
floor and template opt-out; exact helper arithmetic; original feeding-context
and zap-wording freezing; private joining-reference reads; outgoing 9734 text/
chat-kind binding; and the typed overlay owner/epoch/role/idempotency boundary.
The treat fixture also proves overlay-only canary delivery before any joining
publication ACK, while public replies still wait for that ACK. The idle publisher
does not contact core or the remote signer when there is no ready work.

These use temporary databases, actual private peer/framing checks, synthetic
signatures and synthetic relay ACKs. They do not demonstrate public relay
acceptance or the household bunker accepting the new client/kinds.

[Deployment 42757896564](https://github.com/lightning-goats/lightning-goats/actions/runs/42757896564)
passed all five jobs. Package job `129636442959` built the binaries, passed
**eight non-skipped CLI checks** and verified the separate inactive archive with
three binaries and 29 files. The new CLI fixture checks overlay-only configuration,
side-effect-free signer configuration checks, distinct client identity and raw
key rejection. Existing browser, nginx, harmless owner, isolated package install
and core systemd fixtures passed too; the latter do not establish installation
of the new membership/payout/signer roles.

[NIP-46 42757896651](https://github.com/lightning-goats/lightning-goats/actions/runs/42757896651)
passed the existing isolated real-nak regression. Its scope remains distinct from
the new member-publisher fixtures and eventual household-client qualification.

[Security 42757896600](https://github.com/lightning-goats/lightning-goats/actions/runs/42757896600),
job `129636442412`, still fails on unignored **RUSTSEC-2026-0459** in
`libsqlite3-sys 0.30.1` (reported fixed at `>=0.39.0`). RSA remains unreachable.
There is no dependency/lockfile change or waiver in this patch.

The [member delivery architecture](../architecture/cyberherd-member-delivery.md)
describes the implemented optional runtime. No production service, credential,
payment, physical feeding or Nostr publication changed during this work.
Feeder stream copy/herd summary, lifecycle notices, roster/OBS and authenticated
interfaces, recovery/retention/dependency remediation, installed qualification
and explicit activation remain required by the [full port](../planning/cyberherd-implementation-plan.md).
Keep the PR draft; this is source/fixture progress, not completion of that goal.
