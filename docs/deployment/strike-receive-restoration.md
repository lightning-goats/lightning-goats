# Strike receive restoration — 2026-11-08

The operator requested direct restoration of `lightning-goats.com` after an
LNbits exploit, selected Strike and native LNURL-pay, and authorized production
testing. This run starts from main `55ed163fed6742fc79d7c5c34977bb69ea90a439` in
`deploy/strike-lnurl-20261108`. It preserves the existing remediation branches.
No CLN/clnaddress, alternative payment rail, or CyberHerd spending is added.

## Credentials and subscription

The daemon uses exactly `partner.receive-request.create` and
`partner.receive-request.read`. The operator installed a new production key using
`deploy/scripts/install-strike-key.sh`; file input is supported with `--key-file`
and a root-owned mode-0600 source. The script verifies an exact encrypted
round-trip and preserves a private encrypted backup on replacement. No secret is
accepted as a command-line value. Read access returned 200; a single unpaid 1-sat
request with a 60-second expiry returned 201. No payment was sent.

Earlier keys successfully accessed the separate Invoices API but received 403
from Receive Requests and subscription administration. Encryption and input were
independently checked; invoice permissions do not establish receive-request access.
Do not repeat key regeneration without checking the exact endpoint permissions.

Webhook administration uses a separate temporary key with only
`partner.webhooks.manage`. The setup helper generates/preserves the encrypted
signing secret locally and never installs its management key in the daemon.
The operator supplied a private setup-key file. Registration and independent
readback passed for:

- URL: `https://lightning-goats.com/api/v1/strike/webhook`
- Version: `v1`
- Event: `receive-request.receive-completed`
- Initial state: disabled until deployment

The subscription ID and actual account credentials remain operator-controlled.
`setup-strike-webhook.py --apply` prepares disabled delivery;
`--apply --enable` first requires the receive service to be active and verifies
unsigned rejection plus recognition of the saved HMAC secret, using an incomplete
event that cannot credit a payment. Only then does it bind/enable the subscription.
Revoke the temporary management key after activation and readback acceptance.

## Source and deployment changes

Targeted lockfile updates address nine current advisory reports and the Tokio
unsoundness warning: rustls 0.23.46, rustls-webpki 0.103.16, webpki-roots 1.0.10,
quinn-proto 0.11.18, and Tokio 1.53.3. The existing unreachable-RSA exception
remains conditional on the repository's reverse-tree check.

HEAD on the invoice callback now returns 405 in both daemon and nginx instead of
creating an invoice or consuming admission capacity. LNURL responses are no-store.

Pinned Fedora nginx 1.30.6-2.fc44, ModSecurity connector 1.0.4-19.fc44 and
libmodsecurity 3.0.17-1.fc44 are selected with official CRS 4.30.0. The CRS archive
URL/digest are in `deploy/waf/pins.json`; no Apache dependency was installed.
`deploy/waf/acceptance.py` exercises real isolated nginx/connector/CRS with harmless
backends. Detection-only and blocking runs are separate. Raw signed webhook body
bytes are preserved; SQL/XSS and deterministic enforcement probes are blocked.
Matched-data logging is disabled, routine access logs omit URI/query/header/body
values, and per-site nginx errors are restricted to critical severity because
the connector otherwise includes request URIs even with `nolog`. Audit/debug and
response-body inspection are off. Inspecting installed packages alone is not
WAF acceptance.

The authoritative live homepage was imported from the existing host, SHA256
`c86ca2bdbd5a5e2056592b4a02d2cc5ff6a3d8f9939356ac6e57fc7bea037d8c`.
Its stream, current Nostr chat and local vendor assets are retained. Retired
leaderboard/NIP-05 UI and ingress are removed. Unverified zap creation is hidden.
Native six-address payment controls are at `/feed.html`; `/overlay.html` is a
separate OBS browser source with ledger-derived credit, same-stream cursor
reconnect, gap reset, bounded presentation and confirmed-only feed animation.
Replayed feeds do not animate and stale weather expires. No XMR/Square UI is added.

`deploy-strike-receive.py` defaults to a non-applying plan. `--install` verifies
the complete release archive and CRS pin, backs up the inactive installation,
installs root-owned code/config and a versioned static tree, then validates the
proposed complete nginx configuration without replacing active ingress.
`--activate` requires both encrypted runtime credentials and shadow mode, starts
the non-admin daemon, checks health, then switches only the Lightning Goats site.
Other virtual hosts, DNS, WireGuard and home configuration remain outside that
transaction. Activation failure closes the affected API routes; it never resets
paid state or automatically restores exploited LNbits proxy routes.

The operator subsequently confirmed LNbits is stopped, the sole new gateway has
been tested physically from this VPS, and requested removal of all obsolete
LNbits nginx/firewall configuration. `retire-lnbits-ingress.py` replaces only
inspected legacy proxy locations with 410 across the Lightning Goats, bolverker
and retired LNbits virtual hosts, preserving unrelated static/TLS/ACME routes.
It also updates the inactive prepared copies and removes the unused ACL snippet.
The complete proposed nginx tree is checked before applying; prior files are
retained only in a root-private backup.

Inventory found no separate active/permanent LNbits port-3002 rule to remove.
Existing public HTTP/HTTPS, SSH and WireGuard rules remain needed. The shared VPN
forwarding/NAT policies belong to existing peers and are not LNbits exceptions.
A separate atomic nftables OUTPUT policy restricts only the daemon UID: gateway
`10.8.0.6:8789`, local DNS, backend loopback replies and public provider/relay
egress; generic private-network and unrelated loopback services are denied.
The service requires its root-managed firewall unit before startup. Isolated
namespace tests cover IPv4/IPv6 private targets, the gateway, public egress,
loopback backend replies, unrelated identities and repeated atomic reload.
This is service containment, not proof of home-enforced protection from a
root-compromised VPS or a forwarded/spoofed hub path.

## Acceptance boundaries

The initial receiving configuration is explicitly **payment-only**: shadow mode
retains credits and disables new physical dispatch and public Nostr. The homepage
and feed controls disclose the feeding pause. Before enabling dispatch, inspect
the actual owner protocol, shared legacy/new arbitration, safety/UUID state and
accumulated credit. A reachable gateway alone does not establish physical safety.
Public Nostr also needs its separate NIP-46 credentials and signer acceptance.

Local automated checks before packaging: format and strict locked Clippy passed;
the locked Rust suite passed 240 tests, with the dedicated real-nak case ignored
and explicitly unexecuted. The browser suite passed nine mocked tests; deployment
and actual WAF results are recorded separately. These are not live payment,
physical, signer, or genuine Strike delivery evidence. Keep those distinctions
in the final host record. Do not label an unpaid invoice test a settled payment.

The actual source SHA, archive/binary hashes, backup location and installed
configuration are recorded by the root-owned `RECEIVE-INSTALL.json`. Final live
acceptance must additionally record route/overlay/HMAC/WAF checks, subscription
readback, restart behavior, and any operator-observed payment or feed results.

## Live corrections and operator scope additions

The initial public proxy check exposed Fedora's default SELinux label for TCP
8787 (`jboss_debug_port_t`). The host already persistently enables the restricted
`httpd_can_network_relay` policy. Mapping 8787 to `http_port_t` restored nginx
access without enabling broad `httpd_can_network_connect`. The obsolete local
HTTP label override for LNbits TCP 3002 was removed. The installer now records
the required 8787 mapping for repeatable installation.

The first actual LNURL invoice request exposed Strike's one-second expiration
rounding: requested 300 seconds, signed BOLT11 tag 299 seconds, with the correct
description hash. Verification now accepts only exact expiry or one second less,
never zero, any longer duration, more than one second of shortening, an expired
invoice, or excessive future skew. Signature/network/amount/hash checks remain
unchanged. Signed synthetic regressions cover the boundary and late settlement.

The operator requested `herd@lnb.bolverker.com` and the corresponding configured
goat names as aliases. The alias snippet forwards only the native LNURL routes
to the same daemon; discovery retains canonical `lightning-goats.com` callbacks,
recipient username attribution and shared herd credit. Retired generic LNbits,
NIP-05 signup and relay/admin paths stay closed. No DNS change is needed.

The operator separately approved static NIP-05 for all six canonical usernames,
with each goat's existing repository profile rather than mapping all names to
herd. `web/nip05.json` contains public keys only. nginx serves it directly as JSON
with CORS and no redirect at `/.well-known/nostr.json?name=<user>`. This narrow
scope supersedes the older NIP-05 deferral without restoring signup/backend code
or authorizing Nostr profile changes. All six mappings passed live TLS/JSON/CORS
readback and the operator independently confirmed NIP-05 works.

The webhook was enabled only after the actual unsigned-401/signed-incomplete-400
preflight succeeded. Registration and delivery authorization are distinct from a
genuine completed-payment notification, which remains an operator test.
