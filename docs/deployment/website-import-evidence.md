# Authoritative website import — 2026-09-11

Production remains **HOLD**. This is a review candidate, not an activated site.
Base source: `96ee98add789af5d5e5812014100075bb46cc9e0` (draft #47), preserving
the complete remediation stack rather than assuming it is present on `main`.

## Read-only source provenance

The operator authorized `sat` SSH inspections and confirmed the old VPS's
Ed25519 host fingerprint `SHA256:NS0Kp+nD0Os7jYIMgeLthXbX4x5LNun+4A8GGQrGeIw`.
Strict host-key verification used that pin through the inspection namespace.
Nginx identifies `/var/www/lightning-goats.com` as the authoritative static root.
Only its current index and the two referenced public PNGs were imported. Backups,
node_modules, unrelated sites, credentials and legacy application data were not.
Remote scans found no private-key/credential markers in the imported index; its
sole quoted hexadecimal public key matches the approved contact identity.

| Original file | Bytes | SHA-256 |
| --- | ---: | --- |
| `index.html` | 74044 | `d880e262683788f8c5cbda599684b64c55573bd8c1ab5ef66d659e20f2465ac1` |
| `images/preview-image.png` | 1112792 | `b0b36357a3bae0fac478d132a608083c5c9a7d09dcb2fc1762227fa6bc4a813d` |
| `images/lightninggoatslogo1.png` | 339614 | `5379b917380a345cea6e877eb248a89fef4629ad8ca86eda326a82815d3f2ea1` |

The original index is retained privately for comparison; the committed index is
the migration. The PNG bytes are unchanged. The source uses inline CSS and JS;
the unreferenced adjacent `styles.css` and `main.js` were not imported.

## Resulting candidate

- Preserve the source layout, YouTube iframe, Nostr live-event discovery/chat and
  extension/NIP-46 authentication. Extract chat JS into `web/chat.js`.
- Remove the NIP-05 and leaderboard frames and the unverified Nostr zap execution
  path. Restrict the login permission request to chat event kind 1311.
- Replace public email contact with the approved `nostr:npub…` URI and copyable
  public key. This does not choose the daemon signer.
- Add an explicit native invoice form for the six configured users. Default
  `paymentsEnabled: false` prevents even synthetic submissions from fetching
  invoice routes. Enabling it requires separate operator acceptance.
- When enabled for isolated fixtures, discovery and callbacks stay on the page's
  origin with exact native paths, whole safe-integer sats and server limits.
  Reject callback credentials, query/fragment surprises and redirects. Bound
  response bodies to 32 KiB and each request to 10 seconds; no automatic retry or
  WebLN payment. Invoice creation is never described as settlement/feeding.
- Provide copy and wallet-URI actions. The inherited qrcode@1.5.4 build URL
  returns 404, so that dependency and blank QR canvas were removed. QR display
  remains a future UI enhancement; no wallet navigation occurs automatically.
- Include `web/` in checksum-covered releases and fail archive verification if
  the website or its default payment configuration is missing.

The browser's invoice string check only constrains wallet URI syntax. Actual
BOLT11 cryptographic/amount verification remains the daemon's responsibility.
Third-party chat libraries and relays retain their original pinned URLs; their
availability and live authentication are still staging acceptance items.

## Verification and limits

Local environment: new Fedora VPS, Python 3.14.7, Playwright 1.62.0, system
Chromium 152.0.7977.82 with Chromium sandbox enabled. Browser tooling was installed
for development only. No site service or payment/feeder authority was activated.

Five offline browser groups passed: default HOLD/no legacy requests; all six
native address routes including duplicate submission; unsafe/oversized/redirected
discovery; refusal without retries; untrusted chat content rendered as text.
All HTTP and WebSocket traffic is intercepted. The BOLT11-looking fixture is
deliberately invalid; Nostr signature checks are mocked. These tests do not prove
live payment, real signature validation, real relay publishing or video playback.

Initial browser runs timed out on repeated page navigation. The final harness
uses fresh pages and permits 120 seconds for navigation on the small VPS; all
five groups passed in 394.425 seconds. This is not a browser performance pass.
Deployment verification passed all 36 tests, including missing website rejection.
The first sandboxed deployment attempt failed on inaccessible `/var/tmp`; the
authorized isolated rerun outside that filesystem sandbox passed.

Reproduce browser checks using Playwright's installed Chromium, or set
`LG_CHROMIUM=/usr/bin/chromium-browser` for the tested Fedora browser:

```sh
python -m pip install playwright==1.62.0
python -m playwright install chromium
python -m unittest discover -s tests/website -v
python3 -m unittest discover -s deploy/tests -v
```

The Deployment artifacts workflow also runs the offline browser checks on its
disposable runner. Exact commit checks and resulting artifacts belong in the PR
description; they do not authorize production cutover.

Still open: reviewed HTTPS installation on `feeder.lightning-goats.com`, visual
comparison/screenshots, real stream and signer/chat behavior, actual same-origin
daemon integration, browser overlay replay/heartbeat, runtime ownership checks,
operator acceptance and final cutover/rollback rehearsal. No production DNS,
credentials, owner contract or old-VPS availability was changed by this import.
