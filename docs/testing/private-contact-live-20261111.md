# Private contact — actual live deployment, 2026-11-11

The public index now serves the private contact form. Nostr users can send a free
identity-bound NIP-17 DM through their selected signer. Visitors without login
must supply email and pay the separate 100-sat contact fee. That fee has its own
settlement/delivery purpose and adds no feeder or reward credit or public message.
Actual fee payment and recipient-side DM observation have **not** been performed.

The core remains active with 28 sats, zero feeds due, no unresolved feeding and
the operator's override ON. Existing core and membership services are healthy;
both outgoing roles remain masked and every CyberHerd financial ceiling remains
zero. Membership Nostr/lifecycle/rewards/operator routes stay off. This additional
website delivery does not complete the [twelve-part port](../planning/cyberherd-implementation-plan.md).

## Source and qualification

Contact-only configuration source `8071d36e77aa664d159c3917c5d410843cb04bb1`
passes all ten workflows, including
[CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42952149068),
[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42952148997),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42952149102)
and [real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42952149008).
Tested merge `491f5357955ace4cc1b6d032d24e0b45ef5772af` has zero file differences.
Retained systemd artifact `13825961251`, ZIP SHA256
`177504e96a22c8ad611364e189a30b68f1c1c31c4d6be0b3ae2fd6e763e9a2fa`,
passes all six internal checksums. Its explicit contact report proves actual
native configuration checking, current four-journal backup, unchanged live images,
actual active-core/member resumption and malformed-request refusal with zero
gateway POSTs. It explicitly does not qualify real signing or paid delivery.

The corrected static installer source is
`cade21ef0fce8eaf783c7884750943ae42056a4b`; all ten workflows pass:
[CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42954094146),
[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42954094148),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42954094050)
and [real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42954094137).
Tested merge `dee498ddb8035dfa18c476848dff5ff3d7ed3c5c` has zero file differences.
Locked format, strict Clippy, all-feature/native tests and mandatory separate root
fixtures pass. Deployment runs 265 ordinary Python cases with explicit root-only
skips, then all 82 protected upgrade cases separately as root without skips.
Browser/native crypto, nginx/WAF, packaging and installed systemd also pass.

## Actual contact configuration

The [configuration-only controller](../deployment/private-contact-activation.md)
uses a new protected twenty-one-file generation. Every file was independently
matched to GitHub source; later commits' unchanged files were also checked through
provider comparisons. External launcher SHA256 authentication precedes isolated
Python and closure validation. No writable-checkout code executes as root.

The native checker is the separately qualified `9a7c093` artifact with SHA256
`45acfb90165e1ba9bc6388e933b3443072ecd57f0c54023875c1521eb7fb798f`.
It validates both actual configurations and permits only the added contact table.
The recipient remains the existing project identity. Inbox relays are nos.lol,
Primal and Damus, matching signature-verified public kind-10050 event
`defe6a8571ae87261278bcae968f88d821e304930e7fad5e7e486f46e1fb20d2`.
The lookup and offline verification publish no event and load no signing key.

Contact controller receipts:

| Evidence | SHA256 |
| --- | --- |
| Tool manifest | `bc882dce619318fc08295ae124552cce12acba521eace35cf420a776c09fe612` |
| Reviewed preparation | `e8077187763e2688608c17e6e024781ba3d9795b9773d95546b5730051c6a45d` |
| Configuration transaction | `6d41997233a6009e6d21e2d2903b0cad2625744583313c352a71bfc25b9aa798` |
| Current native backup manifest | `6e9a4f1f553fca2f1787bc619e703e3ec0a1f8b88dceb1b5fbc7506372a0a9a5` |
| Applied receipt | `a589e5f18fb71a8125d0be89f8630a34b581446d84a2f7a217f5b3fb8f2691db` |
| Resumed receipt | `8d743022f1c4c2492ad3bfefdaa55f458e8679c82b0fb08a24cbda3ec74d0018` |

All four writers were stopped/masked for the fresh native schema-24 backup.
Independent readback verifies accepted backup receipts, unchanged main/WAL/SHM
images and programs, three initially empty private tables and no pending feeds.
Only the contact configuration was replaced. Separate verified resumption restores
the original unit and member mask state and starts only core plus membership.
No schema migration, database replacement, stale rollback or credential rotation
occurs in this configuration step. The earlier
[schema upgrade](private-contact-schema-live-20261111.md) remains its own record.

## Static switch and actual public acceptance

The first static switch passed nginx syntax checks but returned 403/404 for the
new public files. The exact previous site was restored promptly with no application
restart or financial change. Its private `PUBLIC-READBACK-RESTORED.json` retains
the failure, and its now-unused snippet is preserved privately. The new directory
and changed files had inherited `var_t`; the serving worker still ran in `httpd_t`.
The corrected installer retains the existing serving labels on directories,
new/changed public files and the nginx snippet. It does not disable or alter MAC
policy. Independent label readback precedes the new switch; actual HTTP success,
not a status probe in the caller namespace, establishes serving acceptance.

The current root is
`/var/www/lightning-goats-releases/contact-cade21ef0fce8eaf783c7884750943ae42056a4b`.
The previous release remains intact. Exactly four browser files change:
`index.html`, `site-config.js`, `contact.js` and `contact-fee.js`; all 26 other
original assets retain their exact fingerprints, including OBS/artwork/vendor
files. Existing payment/feeding/readonly-roster flags stay true, operator auth
stays false, and only `contactPaymentsEnabled` is added as true.

Static tool manifest SHA256:
`032c83b7e3ffadd0e87027a93c263898d035eae8129fc1034b36e5e2d9fd1ff7`.
Reviewed preparation:
`14dc71c9e385b8d1d0a16c14ccd91d082215331f3874bdb2d12521675e3fbd1c`.
One-file nginx transaction:
`154f240726a0f55b49eeec20a9573bc522e0af2f476f0bdb9df477dbde8c26df`.
Activated receipt:
`2dfe3056a8410b534bde16f5f5e8809f4742ed18934b88b6ded14bf924b4d7d9`.
Only the site root and exact optional contact include change. Nginx is tested
before/after replacement and reloaded; application PIDs remain unchanged.

Actual public HTTPS verifies nine reads at 200: index, both contact scripts,
deployed flags, leaderboard, roster API, herd LNURL discovery, herd NIP-05 and
OBS. LNURL still advertises native zaps. Five harmless malformed requests verify
native 400, wrong-origin 403, query refusal, invalid status refusal and WAF 403.
Private routes retain no-store responses and the existing log/body protections.

Actual Chromium desktop/mobile verifies the contact modal, required email and
message, free-Nostr option, enabled 100-sat caption, empty-form refusal before
API access and zero page errors. The invisible QR target remains square, hit-testable,
borderless and exactly proportional to the underlying 180/1920 video QR.
The browser blocks POST/invoice attempts and records zero such attempts.

Core PID 1466719 and member PID 1466745 remain running with zero automatic restarts;
nginx master PID is unchanged. Core boot enablement is preserved; membership
remains disabled at boot. Payout and signer remain masked with zero PIDs. No test
payment, synthetic credit, DM, public note or physical feeding was initiated.

Still required: controlled free-signer and paid-contact recipient observation,
including actual authoritative settlement/delivery recovery. A relay acceptance
does not mean read. Wider CyberHerd/provider/recovery/boot-persistence qualification
and explicit later financial/publication activation remain separate.
