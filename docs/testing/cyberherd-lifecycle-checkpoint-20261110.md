# Lifecycle notice checkpoint — 2026-11-10

Feature code `f16bc7c423a7c1d4785b93606946d09afd734f8f` was tested as PR merge
`3f18d41cfce5afc087da82804b8e786881d2f491` into
`18e0261add12d78b6e567f471bb310ba8aec3898`. The local feature tree and the GitHub
merge commit's tree independently match
`05c1e9744c014c0c1fc1d56d72f129611a4695fc`. The review archive records that tested
merge checkout. The current upstream explanation was fetched again and its UTF-8
SHA256 still matches the full plan's pinned
`e56c6793ed0687e4fcac7df54dac48a375673308dc65e988e0dcd987af46e67a`.

[CI 42761271076](https://github.com/lightning-goats/lightning-goats/actions/runs/42761271076),
job `129647252660`, completed successfully: format, strict locked all-target/
all-feature Clippy and the full locked all-feature suite, **449 passed, zero
failed, one ignored**. The separate inactive cross-host example passed two tests
and its non-applying artifact check; those two are excluded from 449. The first
candidate failed a Clippy boolean simplification; the final source corrects it
without a lint waiver. Completed final logs explicitly include all ten additions:

- Quiet initial OFF; unknown readings do not resume or advance the baseline;
  restart/lost-ACK recovery and independent per-relay transition ordering.
- Initial ON announces once; repeated observations do not repeat it; category
  deletions survive default reseeding.
- Stream-only invitations freeze capacity and original chat destination;
  operator pause cancels pending invitations without blocking the pause notice.
- Invitations require an authenticated current live head; an older live update
  cannot resurrect an ended stream; template opt-out is retained.
- Evening notices can select an authenticated stream ended within 15 minutes,
  without restoring its membership/live eligibility; older ended streams get no copy.
- Mountain-time fallback and fresh solar daylight/radiation/sunset precedence;
  morning low light, malformed optional values and stale solar context.
- Actual private-IPC publisher waits on unknown, publishes a pause while override
  is ON, and retains the exact signature/event ID after a lost relay ACK.
- Full outbox backpressure rolls back the transition without prematurely changing
  the confirmed baseline or losing the future notice.
- Per-stream two-hour invitation boundary survives reopening; stale/non-public
  observations are quiet; delayed ACKs extend cooldown beyond queue time alone.
- Real isolated gateway process exposes sanitized fixed Astro reads, rejects
  POST/proxy paths, and sends no OpenHAB command.

Temporary databases, loopback mock OpenHAB, real kernel peer/framing checks and
synthetic signatures/relay ACKs provide this evidence. No production household
Item, credential, public relay, physical feeder or payment is used by these tests.

[Deployment 42761271149](https://github.com/lightning-goats/lightning-goats/actions/runs/42761271149)
passed all five jobs. Package job `129647253477` built all release binaries,
passed **eight non-skipped CLI checks** (including the lifecycle signer/activation
validation assertions) and verified a disabled three-binary, **30-file** review
archive with `activation=none`. Existing mocked website/nginx, harmless-owner,
isolated package-install and core systemd fixtures passed. They do not establish
installed qualification of the new membership/payout/signer identities or the new
household Astro endpoint.

[Real NIP-46 42761271083](https://github.com/lightning-goats/lightning-goats/actions/runs/42761271083),
job `129647253020`, passed the existing isolated synthetic bunker/relay regression.
It does not prove that the household bunker authorizes this new optional client's
kind-1/1311 publications or that the selected public relays accept them.

[Security 42761271328](https://github.com/lightning-goats/lightning-goats/actions/runs/42761271328),
job `129647257583`, still fails solely on unignored **RUSTSEC-2026-0459**, reported
in `libsqlite3-sys 0.30.1` with upgrade `>=0.39.0`. RSA remains unreachable. No
lockfile/dependency update, exception or advisory suppression was added.

See the [lifecycle architecture](../architecture/cyberherd-lifecycle.md). This is
source/artifact progress within the [full twelve-part contract](../planning/cyberherd-implementation-plan.md),
not full-port or production completion. Feeder herd summary/stream copy, readonly
roster/OBS and authenticated interfaces, recovery/retention/dependency remediation,
installed role/provider/household-client qualification and explicit activation
remain required. Keep PR111 draft and execution disabled.
