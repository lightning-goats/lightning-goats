# Public signed CyberHerd self-status — deployed, 2026-11-11

Visitors can now connect a Nostr signer at
<https://lightning-goats.com/leaderboard> and read their own membership status.
Only the exact self-status GET route is public. The operator allowlist remains
empty, operator APIs remain closed, and rewards, membership Nostr/lifecycle and
payout execution remain disabled. This follows the
[completed backend transaction](cyberherd-self-read-backend-live-20261111.md).

## Qualified source

Source `e6110f1b0b902b0bdbea54987835f70388092160` passes all ten workflows:
[CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42992443412),
[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42992443750),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42992443402),
[real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42992443474)
and six home/network workflows. Formatting, strict locked Clippy and the full
native suite pass. Deployment runs 305 general cases with 82 environment skips,
then all 110 separate protected-root cases, including six new site transactions.
All 41 website browser cases pass. Local assembled self-only nginx tests pass all
three cases; the actual ModSecurity/CRS stack passes thirteen isolated cases.

Installed systemd job `130431147816` passes genuine native signed self reads
through the shipped self-only TLS/nginx proxy, nonce replay rejection, closed
operator ingress, native query refusal and unchanged membership. Its private
headers/bodies remain absent from logs. Existing backup, resumption, operator and
restart phases also pass, preserving core credit/original unknown UUID with zero
gateway POSTs, provider operations or publications. These are isolated synthetic
services; actual public acceptance is recorded below.

Artifact source `5d6c627925b30bf48cdf4ca8b1ff304d5e0f7599` independently
compares with zero file differences. Systemd artifact `13842114120` has ZIP SHA256
`8e561d6f7656a18b2b39048cecf0f48ddeeb3b81c7601dc6320bb545408b1d52`.
All six internal checksums and the exact wrapped positive self-read assertions
were verified as data. The first verifier expected only the inner client fields
and refused the additional harness privacy/network fields; checking the complete
committed wrapper accepted the unchanged artifact. No assertion was waived.

## Actual nginx-only deployment

All 23 tool files, two site payload files and four retained browser files
independently matched GitHub blob SHA1, SHA256 and local bytes before root use.
The fresh root-owned tool generation is `lightning-goats-upgrade-e6110f1`.
Its manifest SHA256 is
`75cff740ce0f3cc1e33de3b6d124728eca7867db7e35e0ab525c64f3505ac5d1`;
launcher SHA256 is
`5a039e6db57241679ab6005ef0f5c2a7be02b98c8b0e882389c405dfffba263a`.
External launcher authentication preceded isolated Python interpretation and full
closure verification. No checkout code ran as root. Older generations remain.

The site source manifest SHA256 is
`751c2dddb42df3c43bc4e6762f1cbde8060d4255c346dc91bb8acbea750e3db6`.
Private transaction `self-status-e611-site` remains under the protected upgrade
base, with its payload in `self-status-e611-payload`.

| Record | SHA256 |
| --- | --- |
| Preparation | `4ccae4b9b240c647bc5d1afc489d03a93c4d745c4d1777433c0ea2459f981964` |
| nginx transaction manifest | `4866bca50dfe8942198b5d6921fe185296832c1b3d45f8a18e91edd861b85036` |
| Activation receipt | `92463b0661179650aa87cc2273ced5d057b303fb104cfa9e94d90813c707fdd1` |

Preparation cloned all 30 existing public files. Only `cyberherd-auth.js` and
`site-config.js` have changed hashes; all file metadata and directory serving
labels are retained, including SELinux labels. The client now describes read
failures without a command warning. Only `cyberherdAuthEnabled` changes from false
to true. Existing payment, feeding, leaderboard and contact flags stay true;
index/contact/payment/vendor/OBS/artwork bytes stay exact.

The active root is
`/var/www/lightning-goats-releases/self-read-e6110f1b0b902b0bdbea54987835f70388092160`.
Exactly one nginx site replacement selects it and adds the fixed self-read
snippet. Validation passes before/after replacement, then only nginx reloads.
All 17 original managed application files/configurations, runtime identities and
PIDs remain exact. Core PID 1492476 and member PID 1492501 stay active/enabled;
payout/signer stay masked/inactive. Applications and journals were never opened
or restarted by this site transaction. No database backup/restore/migration was
needed for a static/proxy change. The old release/site copies remain retained.

## Actual public acceptance

Seven verified-TLS HTTPS reads pass: homepage, leaderboard, updated auth client,
no-store site flags, roster, herd LNURL discovery with zap support and overlay.
Three operator API GETs return 404. A public overlay WebSocket independently
returns 101 and its version-1 snapshot; no application frame is sent.

Sandboxed Chromium at 1440x900 and 390x844 loads the actual public page and shipped
browser assets. A known public synthetic NIP-07 identity signs genuine requests;
the real member verifies them through public TLS/nginx/WAF. Both self reads return
200/no-store, inactive membership and `operator=false`, with unchanged revision 4
and cycle 2. Operator controls stay hidden, logout clears self-status, and no raw
nsec/password input appears. Replaying an accepted authorization returns 401 with
no-store. Page errors and attempted payment/command HTTP requests are zero.

The initial browser diagnostic hung; a bounded rerun also timed out. A passive
page load passed. The final diagnostic uses sequential response inspection and an
inert local WebSocket interceptor instead of immediate socket closure; the latter
can provoke login-library reconnect loops. Both desktop/mobile checks then pass.
No production code or browser sandbox/certificate validation was weakened.
Actual relay traffic is excluded from this diagnostic; it does not qualify an
operator's real extension approval or household Nostr Connect handshake.

Post-deployment status retains override ON, 28 sats, threshold 1,000, zero feeds
due and no unresolved feeding. All original application files/PIDs and 30 released
files were independently rechecked. No invoice, payment, injected credit, DM,
public Nostr note, membership command or physical feeding was initiated for these
checks. Normal self-read nonce writes occur in the running member journal.

The full twelve-part port remains incomplete. Actual operator access requires
the selected public-key allowlist; actual zap/contact recipient observations,
network/recovery/provider qualification and operator-selected financial activation
remain separate. PR111 stays draft and the full goal stays active.
