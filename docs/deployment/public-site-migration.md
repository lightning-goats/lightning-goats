# Public website migration for the new VPS

`https://lightning-goats.com/` is part of the Phase 1 cutover surface and must move to the new VPS alongside nginx and `lightning-goatsd`.

## Current public behavior to preserve where applicable

The current public page provides:

- Lightning Goats live stream;
- Nostr live chat and browser-extension login;
- livestream zap/payment controls;
- goat-feeder explanation and `herd@lightning-goats.com` promotion;
- CyberHerd leaderboard UI;
- NIP-05 verification UI;
- contact UI.

The new page should be migrated from the existing production source rather than rewritten from screenshots/content extracts. Obtain the authoritative current `index.html` and static assets from the old VPS before production cutover and place them under version control in this repository (preferred path: `web/`).

## Required Phase 1 changes

### Remove legacy NIP-05 functionality

Phase 1 does not provide NIP-05 verification. Remove:

- the `NIP-05 Verify` navigation control;
- the NIP-05 modal/iframe;
- requests to legacy LNbits/NIP-05 endpoints;
- copy encouraging users to obtain or verify `@lightning-goats.com` NIP-05 identities.

This does **not** affect Lightning Addresses. `herd`, `dexter`, `rowan`, `cosmo`, `newton`, and `nova` remain Lightning Address users handled natively by `lightning-goatsd`.

### Contact

Prefer a Nostr-native contact method rather than exposing an email address in the public page. The Phase 1 implementation should use one configured project/operator Nostr public key and provide a clear `Contact via Nostr DM` action/reference. Do not embed or request an nsec.

The operator approved this contact identity on 2026-09-11:

- Public key: `npub1v60thnx0gz0wq3n6xdnq46y069l9x70xgmjp6lprdl6fv0eux6mqgjj4rp`
- Hexadecimal: `669ebbcccf409ee0467a33660ae88fd17e5379e646e41d7c236ff4963f3c36b6`
- Contact URI: `nostr:npub1v60thnx0gz0wq3n6xdnq46y069l9x70xgmjp6lprdl6fv0eux6mqgjj4rp`

Local validation passed the Bech32 checksum, `npub` prefix, canonical padding,
32-byte decoded length and valid secp256k1 x-coordinate. This establishes encoding
validity; operator approval supplies the intended contact identity.

Use the [NIP-21 URI](https://github.com/nostr-protocol/nips/blob/master/21.md)
with a copyable public-key fallback. A profile URI does not guarantee that every
client opens a DM composer; verify the contact flow in the actual browser/client.
This key is approved for website contact only and does not select or authorize
the daemon's NIP-46 signer.

Authoritative website source and assets still require authorized access. The
identity decision is complete, but the link has not been installed or browser
tested. Preserve the source-import and staging acceptance gates below.

### CyberHerd leaderboard

CyberHerd functionality is Phase 2. The existing leaderboard must not continue calling the legacy LNbits/CyberHerd extension after Phase 1 cutover.

For Phase 1 either:

1. hide/remove the leaderboard control, or
2. retain a clearly disabled/non-live placeholder with no legacy backend requests.

Restore live leaderboard functionality only when Phase 2 provides the replacement API/event source.

### Payment and zap controls

Do not retain LNbits-specific payment URLs. Public Lightning Address payment UX must resolve through the native `lightning-goatsd` LNURL routes.

The Nostr zap flow may remain only if its invoice/payment path works without LNbits/CLN and is verified against the new architecture. Otherwise hide it for the initial Phase 1 cutover and re-enable after validation.

### Live Nostr chat

Preserve the browser-extension-compatible Nostr connection and public live chat if it remains independent of LNbits/NIP-05. The page must never request or accept a raw nsec.

## Deployment model

The static site is served directly by nginx on the new VPS. `lightning-goatsd` remains bound to loopback and nginx proxies only the explicitly required dynamic routes (`/.well-known/lnurlp/*`, `/lnurlp/*/callback`, Strike webhook, status, and overlay websocket).

Suggested filesystem layout:

```text
/var/www/lightning-goats/
  index.html
  images/
  ...static assets...
```

The deployed static tree should be root-owned and not writable by the `lightning-goats` runtime account.

## Staging and cutover

During parallel VPS staging:

1. serve the migrated page on the temporary staging hostname;
2. ensure all assets and stream/chat behavior load without mixed-content or legacy-host calls;
3. verify the six native Lightning Addresses separately;
4. verify no NIP-05/LNbits/CyberHerd legacy requests occur in browser network activity;
5. verify contact exposes only the intended Nostr public identity;
6. verify static files are not writable by the runtime service account;
7. only then include the site in the DNS cutover.

The old VPS remains the rollback source until the new public page, LNURL, Nostr/overlay, and feeder canary checks pass.
