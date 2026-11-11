# Current-schema core update — actual deployment, 2026-11-11

The [feed-intent correction](feed-intent-contention-20261111.md) passed all ten
workflows on source `55ed77b8a3f0c7cb50b031536cb53161de6d5e89`. Production
now runs the corrected core, retaining schema 24 and private contact. The older
schema-23 upgrade controller was not reused against the current live databases.

The [program-only maintenance mode](../deployment/current-schema-core-patch.md)
passes all ten workflows on source `640331c178bfe1214a2911e708a21ad5c607b006`:
[CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42963063083),
[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42963063080),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42963063150)
and [real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42963063067),
plus six home/network workflows. Formatting, strict locked Clippy, native tests,
273 ordinary deployment cases, all 90 separate protected-root cases, browser,
packaging and installed systemd pass. The eight new current-schema cases pass
in the isolated privileged run; six deliberately skip in non-root runs.
Artifact source `e87b99ab7e16190545de036bd87689197424fdd3` has zero file
differences from the branch head, independently checked through GitHub.

## Retained isolated evidence

| Artifact | ID | ZIP SHA256 | Internal checksum entries |
| --- | --- | --- | --- |
| Core package | 13830217597 | `a56cd2fb94894d2736985ce1743095d3eee30d7925c8d4c9bfc7c90d0ac759fd` | 6 |
| CyberHerd review | 13830028363 | `8d49c3fad9c5fb145d646da9aa928d3270fc0ba1c912a3ce6431e403d186bf8c` | 1, then 104-file review verification |
| Installed systemd | 13829459047 | `30e5e6b92d8aa6725cdac1a3fb64ff3354415fddf44f1b5cfe3d99125bcfef7e` | 6 |

The native/systemd report positively verifies the actual current four-journal
backup, both compiled-schema verifiers, original-UID old/new status comparison
on a separate copy, exact unchanged live database images, retained contact,
actual core/member resumption and masked outgoing roles. Its gateway POST count
is zero. This phase uses the same build as installed and selected programs;
different-byte replacement is separately covered by protected-file tests and the
actual VPS readback below. The fixture remains synthetic and loopback-isolated.

Core archive SHA256:
`66d41c30dba792e7301d6c50ba8989f2733263cd24717c06bec53b2fa2588393`.
Review archive SHA256:
`584ad8b61cf4d8651036bf01d27cb748026b4230ba3a209ad314a40c0d173c9c`.
Both ZIP digests, their manifests and full source-specific inner verification
passed as data before payload execution. No test-only binary was installed.

## Actual protected transaction

All 21 tool files independently matched their pinned GitHub blobs before a fresh
root-owned generation was installed. Tool manifest SHA256:
`0730fdcc7f38d347ea4f10f059ff80a43b0e44162783705db66d8fbac6cb4780`.
Launcher SHA256:
`d10e0a8271fc99688d4456fc5043bcfcd177bde313e5debafca7fcac7a80614d`.
The launcher was authenticated externally before isolated Python loaded its
verified closure. Checkout code was not executed as root.

Fresh protected review `core-patch-review-640331c` and execution
`core-patch-execution-640331c` are retained under the upgrade base. Preparation
exercised only CLI help as the original UID in a new network namespace, opened
no journal and preserved all 17 original file identities and both live PIDs.
Independent pre-apply readback confirmed override ON, 28 sats and zero feeds due.

| Retained record | SHA256 |
| --- | --- |
| Preparation | `89b0bb3c293e5839fcfc868fdecc2a7ddb39319fb56c460f5f72577e37bde991` |
| Execution | `ca26e885d7e55bf10fc8f39a58b8425dd1b317d3177e4b799500fee02869a1cb` |
| Three-program manifest | `77578e3ec9ecd0e617b6c1a00c623d380f41ad66e05e8d76cb3b81f19d375a5f` |
| Current native backup manifest | `4215df5e033a67afd27a6c95553a10181b4f2906829b21b2071f8a8cf25affe2` |
| Old/new copied native status | `7bc57dda9463449abe108ef22a5a722bbdab796ec5ecbc77f8e7098a5b9de0ef` |
| Accepted comparison | `b7a6411ce9573e8a8742c2270a58b69787a67057b179c0fa97c4620c07929dd3` |
| Resumption | `8d743022f1c4c2492ad3bfefdaa55f458e8679c82b0fb08a24cbda3ec74d0018` |

Apply quiesced and masked all four writers, took and verified a current backup
with the original native tool, then required the candidate native verifier to
accept the same exact compiled schemas/checksums. Both CLIs produced identical
status on the separate copy under the original UID with networking/capabilities
removed. Only the three sealed programs changed. Independent readback verified
every live main/WAL/SHM image still matched the stopped originals. No live CLI,
schema migration, database replacement, restore or stale-binary rollback ran.

Resumption reverified the accepted images, both backup verifiers, all program/
configuration/unit bindings and masks before restoring the original unit and
starting only core and membership. Both are active/enabled with zero automatic
restarts; outgoing units remain masked with zero PIDs. The receive PID is 1476354
and the unchanged membership program runs as PID 1476381. This is an ordinary
service restart, not a host reboot test.

| Installed program | SHA256 |
| --- | --- |
| `lightning-goatsd` | `8612c95af50504a791b16b923d824294cce10f87eb1c1b3916edd53a1608726d` |
| `lightning-goatsctl` | `3447f5e4998fd06f5c3f93c5ee878098cbd8349c82d06c5afdf45f0da42baaed` |
| `cyberherd-statectl` | `08d8b025e4dd4abfe200854d319067f78ed963e2d151d27fd32d8a55b736d1ae` |

The running daemon executable matches the installed candidate. All original
nonprogram file hashes and the membership executable remain unchanged. Relative
to the previous installed source `9a7c093`, the runtime source/dependency/migration
diff contains only the feed-intent transaction correction and its regression.

## Readback and limits

Actual status retains 28 sats, threshold 1,000, zero feeds due, override ON and no
unresolved attempt. Six public HTTPS GETs pass: homepage, leaderboard, roster,
herd LNURL discovery with native zap support, overlay HTML and retained site
settings. The public overlay WebSocket reconnects and returns a version-1
snapshot without any application command. One empty contact POST receives the
expected native 400 refusal before invoice creation. No actual invoice, payment,
injected credit, private DM, public project note or physical feeding was initiated
for acceptance. Static site/OBS artwork, contact settings, credentials, financial
policy, nginx, DNS, WireGuard and firewall remain unchanged.

The native operator fixture is separately qualified by the
[installed operator checkpoint](cyberherd-installed-operator-checkpoint-20261111.md).
The full twelve-part implementation remains open. Public membership/lifecycle,
operator ingress and rewards/payout activation are not enabled by this update.
