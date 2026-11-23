# Live read-only CyberHerd site — 2026-11-10

**Active:** [the leaderboard](https://lightning-goats.com/leaderboard),
`/cyberherd`, `/cyberherd.html` and exact `GET /api/v1/cyberherd`. The public page
renders current members, exact points/shares and authenticated note/stream/goal
links. Operator routes/auth, CyberHerd Nostr, rewards and payouts remain off.
The full twelve-part port remains incomplete.

## Qualification and preserved release

All ten workflows pass on gate source
`4ee247c22981f4811b17f9c2ab5fbe8e662e023e`; tested merge
`78646ea1bd90b0a5a8eae7836a68d8249cc5d3d9` has zero file differences:

- [Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42913619126):
  actual nginx route regressions `130170300361`, website `130170300230` and
  installed systemd `130173152771`, plus both other jobs.
- [CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42913619331),
  [Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42913619332)
  and [real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42913619518).

The three actual isolated nginx tests cover exact anonymous reads, preserved
pagination/revision queries, aliases against the retired regex, mutation/operator/
prefix denial and unavailable-backend propagation. All three local and CI roster
browser cases pass, including safe exact rendering, disabled auth, empty-state
clearing and stale/future refusal. The first local fixture lacked private nginx
temporary paths; the correction isolates all paths. An older mocked response
could also age during vendor loading. The first clock correction used the wrong
timestamp unit; explicit UTC datetime fixes that fixture while the application's
freshness check remains unchanged. Failed runs were retained, not counted as passes.

Runtime assets remain the previously qualified
`fb73c1d600673cf814618af7da8f9a9634cde797` bytes. Five selected web files and
the optional snippet independently matched pinned GitHub blobs. The actual live
24-file tree was copied, changing only `index.html`, `site.js`, `site-config.js`
and adding the three roster HTML/JS/auth files. All other 21 files, including OBS,
artwork, NIP-05 and vendor bytes, remain exact. Current payment/feeding flags stay
true, readonly roster becomes true, and auth is explicitly false. The source's
default-off payment configuration was not deployed.

New protected static root:
`/var/www/lightning-goats-releases/fb73c1d600673cf814618af7da8f9a9634cde797`.
The previous `ebe0986` release remains retained. All 27 new files passed root
ownership/mode/hash readback. Manifest SHA256:
`43d2f554053566f46b6e97f246bce412d920912b1faed3bb9663caae0650bb7e`.
New snippet SHA256:
`865e237a8f32780ff823b090e144d7d040a346648b26190e7a41c5559f9be002`.

## Actual activation and host policy

A new protected preparation at
`/var/lib/lightning-goats-upgrades/cyberherd-roster-site-fb73c1d` passed independent
readback. Prepared receipt SHA256:
`63a7d66089ec4a0f5fe50ab3205ce79819bc899b60b2cb43c612b2c40d576b08`.
The existing authenticated file-transaction primitive retained old/new nginx
site bytes/metadata. Transaction manifest SHA256:
`d2e79ac0e7b22e581c0c24100d0a2a5615ca8ee66c56d5934d0d58a2bf1e24c2`.
Only the static root literal and exact snippet include changed in the site.
Original payment/proxy/WAF/NIP-05/retired-path configuration remains intact.
`nginx -t` and reload succeeded, with configuration-only rollback retained.
No application executable, configuration, unit, journal or PID changed.

The first actual public read returned 502 while the loopback view remained 200.
Enforcing SELinux had `httpd_can_network_relay=on`, general network connect off,
and no HTTP label for TCP 3340. Adding **only** that port's `http_port_t` mapping
resolved public reads to 200 without service restart or broader permissions.
No firewall, DNS, WireGuard or general network boolean changed. The precise
apply/owned-rollback commands are in the [runbook](../deployment/cyberherd-readonly-site.md).

The specific AVC search returned no matching audit line. Its assertion failed,
but the shell continued to the authorized narrow port command. An initial private
receipt incorrectly claimed that audit confirmation. The retained
`SELINUX-PORT-READBACK.json` explicitly corrects that claim and records the actual
before/after HTTP and boolean evidence. No unobserved audit denial is claimed.

## Actual public acceptance

- Both aliases reach `/cyberherd.html` successfully; the page and API return 200.
- Sandboxed Chromium renders Destiny and Sat, their exact 4/1 points and 8%/2%
  projected shares, capacity and all three Nostr target links, without page errors.
  Login/operator controls remain hidden and all observed browser requests are GET.
- The API is no-store. POST returns 403. Operator API/page return 404. Plain
  unknown query data returns application 400; a script-injection query returns
  WAF 403. WAF rules were not relaxed.
- Herd LNURL discovery still returns 200 with native zap support. The unchanged
  OBS files retain the already verified member/card and payment/feed behavior.
- Core PID `1434299` and membership PID `1437779` stay unchanged with zero automatic
  restarts; outgoing roles remain masked/inactive, PID0.

No invoice, payment, credit grant, test Nostr publication or physical feeding was
initiated. Shares remain projections, not executed payouts. Remaining acceptance
includes incoming paid zap/receipt, boot persistence, operator interface and later
public membership publishing, controlled provider/payment/financial qualification
and externally reconciled recovery. Keep PR111 draft and the full goal active.
