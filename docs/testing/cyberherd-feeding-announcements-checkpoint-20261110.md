# Confirmed-feeding presentation checkpoint — 2026-11-10

Feature code `a2270628d11166d72ecb4a9ae12fa1055c6a6407` was tested as PR merge
`0f520ea9d01677c7fb343c9a3de738a7859d1aef` into
`18e0261add12d78b6e567f471bb310ba8aec3898`. Independent local/GitHub Git-data reads
confirmed both trees equal `cb1fd0d76099f49673a7a2f279bc7b7b4fc29f31`. The review
archive records that actual tested merge checkout.

[CI 42765685222](https://github.com/lightning-goats/lightning-goats/actions/runs/42765685222),
job `129661512900`, completed format, strict locked all-target/all-feature Clippy,
and the full locked all-feature suite: **458 passed, zero failed, one ignored**.
The separate inactive cross-host example passed two tests and its non-applying
artifact check; those two are excluded from 458. The preliminary candidate's
new ambiguity fixture expected an empty result where the core correctly returns
an error. The final fixture checks that stronger existing refusal contract; the
core's actuation/ambiguity behavior was not weakened.

Completed final logs explicitly include all nine new regressions:

- A 10-sat credit crossing 995 sats, followed by another 200 sats during dispatch,
  retains 10 as the triggering amount, debits 1,000 and leaves 205. The feeding
  notice freezes the earlier 34-point member snapshot despite a later increase.
  One standalone note and one original-stream chat are queued; later authenticated
  stream replacement cannot retarget them. No fake joining/member tags appear.
- A single 2,345-sat payment produces two confirmed feeding/reward barriers and
  one announcement after reopening. A distinct later payment remains distinct;
  immutable presentation decisions cannot be deleted to replay it.
- The real private-IPC overlay requires actual food proof, epoch, owner and UUID.
  Ordinary credit cannot certify food. A lost ACK returns the original sequence;
  changed content/identity/epoch/UUID and the payout role are rejected. Import
  appends only one text/artwork event and no additional food, debit or core Nostr job.
- Canary publication waits without signing. Public-mode confirmed food remains
  announceable after a pause, with the same signature/event bytes on relay retry.
- Default nondelegated operation retains the existing core note/overlay behavior
  and creates no duplicate CyberHerd notice.
- Template opt-out remains a durable per-payment decision across multiple
  confirmations; successful discovery with no live stream invents no chat root.
- Funded/uncertain food creates no feeding notice or debit and prevents another
  automatic request under the existing ambiguity rule.
- Exhausted bounded attribution uses an editable neutral confirmation template,
  without inventing a threshold-sized or zero-valued payment.
- Equal wording/timestamps from distinct feeding sources still produce distinct
  native signed Nostr event IDs through their source-hash tag.

These fixtures use temporary ledgers, synthetic credits/signatures/relay ACKs,
actual kernel peer/framing checks and simulated confirmation calls. They do not
send a physical feeder request, real payment or public relay event. Existing
settlement/feeder/reward/result/member/lifecycle tests also passed in the full suite.
A preliminary local strict-Clippy run completed successfully in 16m33, but source
changed during that run; the exact final CI checkout above is authoritative.

[Deployment 42765685080](https://github.com/lightning-goats/lightning-goats/actions/runs/42765685080)
passed all five jobs. Package job `129661512003` built all release binaries,
passed **eight non-skipped CLI checks** and verified a disabled three-binary,
**31-file** CyberHerd review archive with `activation=none`. Existing browser/nginx,
harmless owner, isolated package-install and core systemd fixtures passed. They
do not establish installation of the new roles, household configuration or the
new optional core presentation policy.

[Real NIP-46 42765684976](https://github.com/lightning-goats/lightning-goats/actions/runs/42765684976),
job `129661508129`, passed the existing isolated real-nak regression. It does not
qualify the actual new household client or selected public relay destinations.

[Security 42765684791](https://github.com/lightning-goats/lightning-goats/actions/runs/42765684791),
job `129661507753`, still fails solely on unignored **RUSTSEC-2026-0459** in
`libsqlite3-sys 0.30.1`, reported fixed at `>=0.39.0`. RSA remains unreachable;
there is no dependency/lockfile change or waiver.

See the [feeding presentation boundary](../architecture/cyberherd-feeding-announcements.md).
The explicit default-off handoff has not been installed or activated. Paid state,
feeding caps/authority, production configuration, artwork, credentials and public
messages were not changed by this source work. The [full port](../planning/cyberherd-implementation-plan.md)
remains incomplete: optional once-per-cycle repost hints, roster/OBS and authenticated
interfaces, restore/retention/dependency work, installed identity/network/helper/
provider/household-client qualification and explicit activation remain required.
Keep PR111 draft and financial execution disabled.
