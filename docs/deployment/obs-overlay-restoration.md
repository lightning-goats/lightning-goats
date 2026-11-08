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
