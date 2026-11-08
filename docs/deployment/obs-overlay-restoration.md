# OBS overlay restoration — 2026-11-08

The operator supplied the existing live HTML as the visual/behavioral reference.
The replacement is `https://lightning-goats.com/overlay.html`; configure an OBS
Browser Source at **1920 × 1080**, with a transparent background. The document
intentionally retains the fixed Full HD coordinate system; a smaller browser
window may crop its right or bottom edge. For a complete browser preview on a
smaller monitor, open `https://lightning-goats.com/overlay.html?fit=1`; the default
URL retains the native OBS coordinates.

## Restored presentation

- Original orange/black surfaces, top-left sats remaining and BTC/24-hour badges,
  top-right 180-pixel payment QR, bottom clock/temperature/progress ticker.
- Centered payment notices with the original amount tiers (1/100/1000/5000/21000
  sats), near-threshold emphasis and a separate two-second QR/lightning swap.
- Scrolling messages, progress clipping behind moving text, goat portrait cards,
  bounded queues and 18-second card presentation.
- Only a fresh `feeder_confirmed` event can produce the full-bar shine, lightning,
  screen shake, feeder announcement and visual drain. Reaching the threshold or
  receiving a payment is never proof that the physical owner completed a feed.
  The visual drain never changes ledger credit; two confirmations from 2340
  sats leave 340 throughout and after the animation.
- Ordered durable stream/resume/reset handling. Reconnect replays update state
  without replaying old payment, feed, QR, card or roster effects. A slow status
  response cannot overwrite newer credit from the ordered WebSocket.
- The clock uses the OBS/browser host's local timezone, matching the supplied page.
  Reduced-motion preferences suppress flash/shake and decorative animation.

The canonical LNURL discovery URL for `herd@lightning-goats.com` is regenerated as
uppercase Bech32 and encoded locally using the existing vendored QR library with
H error correction and a four-module quiet zone. The project logo sits in a
small white-backed center badge. Independent ZXing decoding of the **rendered
180×180 QR including the logo** is part of the browser regression suite. No
payment credential is encoded. There is no GSAP/CDN dependency; animation uses
native browser APIs and CSS.

The original portraits/lightning artwork are still on the operator's OBS host
and will be uploaded later. The project logo is the current portrait fallback;
a native lightning glyph supplies the QR-swap effect. On later frontend upgrades,
`deploy-obs-overlay.py` preserves original uploaded image files when not yet
present in the source release. Add reviewed uploads to `web/images/` as public
assets so future packages carry them directly.

## Weather and price data

A separate non-admin `lightning-goats-display` system service has no Strike,
OpenHAB or Nostr credential and cannot access the private payment-state directory.
Every minute its timer refreshes `/var/cache/lightning-goats-display/overlay-data.json`
atomically; nginx serves only that fixed public JSON file. Browsers poll it every
15 seconds, rather than making provider/home-network requests per viewer.

Weather comes **only** from `http://10.8.0.6:8789/v1/weather`, retaining the actual
`observed_at` timestamp and normalized Fahrenheit units. The writer whitelists
presentation fields and rejects invalid, stale (>5 minutes), excessively future
(>30 seconds) or unit-confused values. The browser independently expires values.
It never reaches OpenHAB `:8080`, the weather receiver `:5000`, or a mutating route.
An identity-specific nftables policy permits only the gateway's TCP 8789,
local DNS and public egress; other private/loopback services are denied. The timer
service requires this policy before starting.

BTC/USD and its 24-hour change use CoinGecko's public
[`simple/price` endpoint](https://docs.coingecko.com/reference/simple-price),
with the provider's `last_updated_at`. Coinbase public stats returned 403 from
this host and are not a runtime dependency. Prices are indicative display data;
they do not set the feeder threshold, grant credit, convert funds or affect
settlement. Missing/stale provider data becomes `--`, not a fabricated value.
The only browser data paths are the same-origin cache, operational status and
ordered overlay socket. Portraits may later use public HTTPS avatars.

The existing 60-second informational scheduler is enabled with its existing
40% interface-information / 40% weather / 20% no-message distribution. Those
messages remain overlay-only. Physical mode and Nostr publication remain as
previously configured; this overlay deployment does not authorize feeding.

## CyberHerd boundary

The frontend can render sorted, bounded ten-member roster cards, newest-member
highlighting, contribution labels, daily reset and roster/event ticker text when
future `cyberherd_*` durable events supply them. Browser fixtures exercise this
adapter, including hostile text/image input. **There is no current roster producer
in Phase 1.** Membership, headbutts and rewards/spending remain Phase 2 and no
old LNbits settings endpoint, key or WebSocket is used. A frontend adapter is not
live CyberHerd acceptance.

## Installation and verification

`deploy/scripts/deploy-obs-overlay.py ARCHIVE SOURCE SHA256` verifies a complete
clean-source package and prints a plan. `--apply` installs the versioned static
site, cache writer/timer and its network policy, validates nginx before reloading,
and enables only the existing informational/weather configuration flags. Payment
binaries, runtime credentials, paid state, DNS, WireGuard and the feeder mode are
preserved. Existing browser payment-enable settings are copied to the new static
root. A root-private backup and `OVERLAY-INSTALL.json` record identify the change.
If changing the informational flags requires a daemon restart, state is backed up
after quiescence. An older financial database is never restored automatically.

Run the browser suite with all HTTP/WebSocket traffic mocked:

```sh
python3 -m pip install -r tests/website/requirements.txt
python3 -m playwright install chromium
python3 -m unittest discover -s tests/website -v
python3 -m unittest discover -s deploy/tests -v
```

QR scanner and image-reader dependencies are pinned test tools, not production
runtime requirements. Record live trusted TLS, socket state, actual weather and
market timestamps, QR decoding, ownership, negative network paths and preserved
credit separately from synthetic animations. Do not submit a real feed merely to
exercise a visual effect.

## Observed initial deployment and operator-directed activation

The initial overlay source was `05cfb425be9f5b394f36892dab85c96dd61a1b2f`;
its archive SHA256 was
`b6d54750bd5741543c70b88b56d0096619d326f8f6baea7e66530b9f11442f2c`.
The root install record points to backup
`/var/backups/lightning-goats/obs-20261108T222550Z`.
Sixteen browser tests passed, including the independent QR decoder; the deployment
suite ran 126 cases with five existing skips. The isolated display-firewall
exercise passed gateway/public access, five private-path denials and atomic reload.

Live Chromium acceptance established trusted TLS, the public ordered ledger,
current gateway temperature, fresh indicative BTC/24-hour data, no JavaScript
errors and independent decoding of the actual 180-pixel center-logo QR. The
1024×576 `?fit=1` preview included both the QR and bottom bar. The separate display
UID could read gateway health but was denied OpenHAB :8080, legacy weather :5000,
home SSH :22, the loopback payment listener, payment-state files and the encrypted
credential store. Payment binaries remained unchanged. Real credit was retained;
subsequent operator payments produced additional settlement/event records. Live
interface-information and weather events appeared while the Nostr outbox stayed
empty. The operator independently tested the new overlay with another real
payment and approved placing it on the actual OBS stream.

The operator then explicitly authorized feeding and confirmed the OBS switch is
the existing gateway-bound OpenHAB `FeederOverride`. The operator switched it ON;
a read-only gateway check observed `override_enabled=true, remote_enabled=true`,
with 3 sats credit, zero feeds due and no unresolved UUID. This is the explicit
physical enablement step; no home Item is changed by VPS tooling.

`enable-feeding.py` defaults to a read-only plan. Its explicit `--apply` transition
requires the local override to remain ON, the existing remote-enable gate ON,
no unresolved UUID, the exact existing ledger/gateway and a private quiesced-state
backup. It arms `canary` feeding while retaining the existing threshold, local
caps, all paid state and Nostr-disabled status. The worker and gateway check the
local override before dispatch; switch changes never fabricate a confirmation or
discard credit. Any existing ambiguity blocks new physical UUIDs. The helper
sends no feeder POST and never restores an older financial database on failure.

The connected overlay no longer displays “Live ledger”; connection failures still
remain visible. The availability display polls the gateway-backed public status
every ten seconds. Override/local-disable/paused mode shows a prominent centered
**“⏸ Feeder Paused”** banner; unknown state also shows a paused warning. When the
operator's override is OFF and the gateway/remote gate permit feeding, the pause
banner disappears. No home switch is mirrored by writing back from the browser.
Seventeen mocked browser tests passed after this update, including the visible
ON/OFF behavior and unchanged credit. A real feed/completion observation remains
separate from arming the paused service.

The update was deployed from `103df1977148660f73d2d1eff643f65b2c83c419`,
archive SHA256
`bc7d18e762cace44e42888e5f17db0257a51241900dbc743170fb9e21c73582c`.
The static-install backup is
`/var/backups/lightning-goats/obs-20261108T223724Z`.
At `2026-11-08T22:37:50Z` the explicit guarded activation succeeded;
`FEEDING-ENABLE.json` records the quiesced-state backup
`/var/backups/lightning-goats/feeding-arm-20261108T223747Z`.
The daemon verified `mode=canary`, override ON, remote-enable ON, retained
3 sats/1000-sat threshold, no feeds due and no unresolved UUID. No helper submitted
a feeder command, and subsequent readback showed zero feed attempts. The public
payment controls now reflect enabled feeding; the homepage no longer claims a
permanent payment-only pause. The payment binary hash remains unchanged.

The operator then exercised the actual OBS switch and independently confirmed
the prominent paused text responds to Item updates. A browser probe that assumed
the switch would remain ON timed out during those live toggles; no failure was
inferred for the feeder/overlay from that observation. The final live check
instead accepts the selected ready/paused state and verifies the corresponding
visible banner plus hidden connection label. The gateway and public status both
exposed the changing local control without VPS writes to it. A completed physical
feed through this service still needs operator observation on a later threshold
crossing; activation, synthetic animation tests and prior gateway-only physical
tests are not substituted for that observation. Public Nostr remains disabled
pending its separate isolated signer/client setup.

## Progress-track appearance — operator follow-up

The operator requested removal of the progress-bar background flashing. The
continuous `container-glow`, fill brightness pulse and striped-flow animations
inherited from the supplied overlay were removed. The idle track and fill are
steady; progress updates, scrolling messages and confirmed-feed effects retain
their existing behavior. The stylesheet cache version is bumped so refreshing
the OBS source loads the correction.

The steady-track correction was deployed from
`e16c3484a5a67997e31b40ea6849ce65b0387ee1`, archive SHA256
`a2856fca779a3830a57dd728e25443bce7f2d286e4787752dfae6c051d557a4a`,
with backup `/var/backups/lightning-goats/obs-20261108T231310Z`.
All 12 existing overlay regressions passed. On the actual public page with
positive live credit, computed animation names for both the track and fill were
`none`, including after the operator-authorized 9/10/25-sat grant sequence recorded
in [operator-credits.md](operator-credits.md). Credit finished at 47 sats, with no
feeding due or physical attempt. Existing OBS sources need a refresh to load the
stylesheet cache version 4. Payment binaries and physical mode were unchanged.
