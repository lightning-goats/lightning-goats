# Bounded bridge admission — deployed, 2026-11-11

Production now runs the bounded FIFO bridge correction from source
`886e6438b83497f0ef4db9931620a21941c676a9`. An admitted request can wait up
to 100 ms for the single database-operation permit. The eight-connection bound,
two-per-second admission budget, peer/role authority and financial fences remain
unchanged. Native real-frame regressions verify waiting followed by the original
batch, bounded timeout, permit release and later successful admission.

All ten workflows pass on this source, including
[Rust CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42973865968),
[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42973865835),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42973865539)
and [real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42973865658).
Formatting, strict locked Clippy, the native suite, protected-root tests, browser,
packaging and installed systemd checks pass. Artifact source
`6830038b2dd2a650c84bf712a9959e59885733cc` independently compares with zero
file differences from the qualified branch source.

## Retained artifact verification

| Artifact | ID | ZIP SHA256 | Internal checksums |
| --- | --- | --- | --- |
| Core | 13833979897 | `8546145bcc3cd1a5cb7116ced9c89c181ffa0f7367d39e68b2228c1899113c72` | 6 |
| Review | 13835076345 | `3fbec060958b0011ed651a7ae8408b8d0e2ddba6151eb1e5b937d148c3b71bf6` | 1, then 104-file verification |
| Installed systemd | 13834618387 | `0060225aaad28da8f13acd16cc4784265af91dc80d3a4009a6f1badc9e0d5080` | 6 |

Core archive SHA256:
`c2cf202b971f2a2241006ab8c6d2bf1a31f10e38d7332820edd6212451c089a3`.
Review archive SHA256:
`2dd2b804d622ffd1e669761faa3d7c762802359c2785e29401ff5f5020aa742c`.
All ZIP and inner manifests passed before payload execution.

The installed report positively verifies the extended operator controls:
activation/manual score, stale revision refusal, ban/explicit unban, exact public
normalized points and zero share, paused controls/manual reset, and original
receipts across two further real member restarts. Core credit and its original
unresolved feeder UUID remain unchanged; gateway POSTs and provider operations
are zero. These are synthetic, loopback-isolated tests, not actual operator or
financial activation. Earlier failures remain recorded in the
[operator checkpoint](cyberherd-installed-operator-checkpoint-20261111.md).

## Actual program-only maintenance

The existing independently authenticated 21-file tool generation `640331c` was
reused after verifying every file still matched the selected source. Launcher
SHA256 `d10e0a8271fc99688d4456fc5043bcfcd177bde313e5debafca7fcac7a80614d`
was checked externally before loading its verified closure; tool manifest SHA256
is `0730fdcc7f38d347ea4f10f059ff80a43b0e44162783705db66d8fbac6cb4780`.
No checkout code ran as root.

Fresh protected transactions `core-bridge-review-886e643` and
`core-bridge-execution-886e643` are retained under the upgrade base.

| Record | SHA256 |
| --- | --- |
| Preparation | `945fd1d0df0de80068045875f89bb962825b8685d4e58ab72f76cf09f47d41a1` |
| Execution | `8d4ce171b9631273d6fb3a0b47ff7e6a3fc6c4ddf0685261c99dfad5f1e80e9e` |
| Program manifest | `c20c3f2ae2b611bd58d318e5c661aaa08a830b3c9faaac536b18f2280ff9967b` |
| Native backup manifest | `4111038b8e63c4f20ea1ca6dc83c6dc4b54c07f7fb84abd76a9d29b8d095f14f` |
| Comparison | `103991644f87533a597dc6d86f6a43a925e45aadc42ddef2a50667ed876b7fad` |
| Resumption | `8d743022f1c4c2492ad3bfefdaa55f458e8679c82b0fb08a24cbda3ec74d0018` |

Both native compiled-schema verifiers accepted the current four-journal backup.
Old/new CLIs returned identical status on a separate copy under the original UID
with networking/capabilities removed. Every stopped live main/WAL/SHM image was
independently checked unchanged before resumption. Exactly three programs changed;
no migration, live CLI, database replacement or restore ran. Configuration,
identities, units, membership program, private contact, static site and artwork
retain their original hashes. An initial independent metadata check omitted
extended attributes; the corrected check included the retained SELinux attributes
and passed. No production drift was found.

| Installed program | SHA256 |
| --- | --- |
| `lightning-goatsd` | `79698cd2c4ef0e1289b63eada3c12fa0e90916b7aa4deb660a083224fe321fdc` |
| `lightning-goatsctl` | `07d0c0f192ee24dc7fb05c80916d5dfcef6ebecdbaa5df23d34c34456526ec1c` |
| `cyberherd-statectl` | `db9fc420f4df7dd7b210e4709eeede41e63eed23e35a1872a10c47976cb8aa85` |

Core PID 1486489 and unchanged membership PID 1486514 are active/enabled with
zero automatic restarts. Payout and signer remain masked/inactive with zero PIDs.
Nginx retains PID 1056130 and was not reloaded. Fresh status retains 28 sats,
threshold 1,000, zero feeds due, override ON and no unresolved attempt.

Six public HTTPS GETs pass: homepage, leaderboard, roster API, herd LNURL discovery
with native zap support, overlay HTML and retained contact-enabled site settings.
The public overlay WebSocket returns its version-1 snapshot without an application
command. No invoice, payment, injected credit, DM, public note or feeding was
initiated. DNS, WireGuard, firewall and production nginx were unchanged.

The full twelve-part port remains incomplete. Actual operator ingress awaits the
operator-selected public-key allowlist. Incoming zap/receipt and private contact
recipient observation, remaining network/recovery/provider qualification and
selected financial ceilings remain separate. Operator auth, membership Nostr,
lifecycle, rewards and payouts stay disabled.
