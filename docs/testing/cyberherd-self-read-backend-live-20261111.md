# Signed self-read backend — deployed, 2026-11-11

The later [public self-status release](cyberherd-self-read-public-live-20261111.md)
now exposes self reads and browser login. This record describes the earlier
backend-only transaction; its public-off observations remain historical.

The member now accepts authenticated self-status reads on its existing loopback
listener, with canonical origin `https://lightning-goats.com` and an **empty**
operator allowlist. Public self-read ingress and the browser auth flag remain
off. No operator key, public operator route, reward, membership Nostr/lifecycle
or payout authority was enabled.

## Exact source and isolated qualification

Source `548276f0f68674073ec583ee7fcffc0237a4c2dc` passes all ten workflows:
[CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42983638237),
[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42983638285),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42983638311),
[real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42983638089)
and six home/network workflows. Formatting, strict locked Clippy, the full native
suite and required separate root fixtures pass. General deployment discovery runs
290 cases with 76 environment skips; the separate protected-root run passes all
101 cases, including all eight new self-read file cases. Browser and packaging
also pass. Artifact source `2a35a0c3e97a612c5daadfc53c4ac4c08abc61b5`
independently compares with zero file differences.

Installed systemd job `130402076022` verifies the actual configuration transaction,
original-UID native checking, current four-journal backup, unchanged paid images,
member-only change and core/member resumption. Two synthetic identities can read
only themselves; operator state/templates/reset are refused, preserving membership
revision/cycle, core credit and the original unresolved feeder UUID. Gateway POSTs,
provider operations and publications are zero. Every earlier operator/TLS phase
also passes in this isolated disposable run.

Retained systemd artifact `13838429033` has ZIP SHA256
`e65d12a98922052ab4dd7ff7868a543e5537f116b7025fd585d39ddf95a044e9`.
All six internal checksum entries and exact named positive self-read assertions
were checked as data before actual-host use. The fixture itself remains synthetic;
the actual deployment is recorded separately below.

## Actual protected transaction

All 22 tool files independently matched their pinned GitHub blobs and SHA256s.
A new root-owned generation `lightning-goats-upgrade-548276f` was installed as
data. Its manifest SHA256 is
`addf8e3999faf9465ab60ce1a28f0d636e7b0dfff3e87ec343e6268277bf0848`;
launcher SHA256 is
`400ed2d8305012e54552c282fc437d3740adabcd870a557549513c73670a2c3d`.
The launcher was authenticated externally before isolated Python loaded and
verified its full closure. No checkout code ran as root. Older tool generations,
credential importers and completed transactions remain retained.

Fresh transaction `self-read-activation-548276f` is retained under the protected
upgrade base. It did not reuse the earlier configuration-check-only record.

| Record | SHA256 |
| --- | --- |
| Preparation | `2d5ab08a8cea6fe511d99aa66fa6f2b50c007524fde073fa3a35bb292b292c87` |
| Configuration manifest | `2a29d50b8f32e90f2e08994edf474aa56cc16ff3f82288db9e7ec2dbfb0ccfeb` |
| Candidate member configuration | `34e8e987833b562528ca6f0539d03b7952e3efb1a5d6ae33eb88cc2d565ffbda` |
| Native backup manifest | `6b93091b820c5b80b452e784aff8901c8e5a022bda6efed9892e808173559647` |
| Applied comparison | `02be4f73786604ad98154babbfc84ca7a79465e2c3dadfae00020682c55fd8dc` |
| Resumption | `f431c7619480b6dd344e29c9e384faf25573f5464af1b7145292cd05685e2acb` |

Independent preparation readback verified all 17 original managed files/PIDs,
the exact one-section configuration delta, an empty operator allowlist, enough
backup space and override ON with 28 sats/zero due/no unresolved feed. The
installed member's configuration checker ran as its original UID/GID without
supplementary groups, capabilities, credentials or networking.

Apply quiesced/masked all four writers and took/verified a fresh current native
backup. Exactly one member configuration changed. Independent readback confirmed
every stopped live main/WAL/SHM image remained byte-identical, all programs and
other configurations remained exact, and all writers were stopped/masked before
resumption. Resume reverified the backup, paid images and native configuration,
restored the original units and started only core/member. No migration, live CLI,
financial-state replacement, restore or stale-program rollback ran.

Core PID 1492476 and member PID 1492501 are active/enabled with zero automatic
restarts. Their executables retain the
[previous bridge deployment's hashes](cyberherd-bridge-turn-live-20261111.md).
Payout/signer remain masked/inactive with zero PIDs; nginx retains PID 1056130.
Private contact, core configuration, all monetary ceilings, credentials, static
site/OBS artwork, DNS, WireGuard and firewall are unchanged.

## Actual acceptance and remaining work

The pinned browser crypto bundle signed NIP-98 GETs using a public synthetic guest
identity, sent only to the actual loopback member. A signed self GET returns 200
and no-store, with the correct public key and `operator=false`; replay returns
401, and a fresh valid guest operator-state GET returns 401. No membership command
or relay message was sent. An initial Node-VM verifier attempt rejected its event
object's foreign JavaScript realm before any HTTP request; creating the template
in the bundle's realm corrected the verifier, without application changes.

Actual status retains 28 sats, threshold 1,000, zero due, override ON and no
unresolved attempt. Six public HTTPS reads pass: homepage, leaderboard, roster,
herd LNURL discovery with zap support, overlay HTML and site settings. Public
self/operator paths still return 404; browser auth remains false and private
contact remains true. A new public overlay WebSocket returns a version-1 snapshot
without an application command. No invoice/payment/injected credit, DM, public
project note or physical feeding was initiated for acceptance. Auth nonce records
are normal membership-journal writes after resumption; the exact-image guarantee
above applies to stopped financial maintenance, not post-resume nonce activity.

The next interface step is exact public self-read ingress plus the browser flag,
preserving all current settings and keeping operator ingress closed. Administrator
access still needs the operator's public-key allowlist. The full twelve-part port,
actual user/browser and incoming-zap/contact recipient observations, remaining
network/recovery/provider qualification and selected financial activation remain
unfinished. PR111 stays draft and the full goal remains active.
