# Static staging acceptance candidate

Production remains **HOLD**. This is a prepared, unactivated website/TLS stage
for `feeder.lightning-goats.com` at `64.177.40.118`. It does not establish payment,
gateway, feeder, signer or full staging acceptance.

## Reviewed scope

`deploy/nginx/lightning-goats-static-staging.conf.example` stands alone in nginx's
HTTP context. Do not enable the application canary site on the same listener.
Only the six reviewed website files are served (the root also serves index.html).
The server supplies its own no-store `site-config.js` with payments disabled.
Unknown files, LNURL, webhook, health/status, overlay and OpenHAB paths return 404;
methods other than GET/HEAD return 405. No upstream is configured. Unknown Host
values return 421. Binding is IPv4 only, on the inventoried new VPS address.

The site's existing external YouTube and Nostr chat remain browser features.
The server never initiates a feed or payment. Browser acceptance must distinguish
viewing the page from deliberately signing/publishing chat or opening a wallet.
No live browser publication is authorized by this candidate.

## Local evidence

Three real-nginx test groups use loopback listeners and a temporary self-signed
certificate. They verify MIME types/assets, forced payment disable despite an
enabled copied asset, rejection of accidentally copied private/retired files,
all disabled application routes, method/Host checks, HEAD, ACME challenge reads,
and fixed-host HTTPS redirects. They do not test public DNS/TLS, firewall ingress
or external browser integrations. Run:

```sh
python3 -m unittest discover -s deploy/tests -p test_static_staging.py -v
python3 -m unittest discover -s deploy/tests -v
```

## Prepared activation sequence — approval required before execution

1. Pin the reviewed merged source or exact draft SHA and its successful artifact
   checks. Verify release checksums/provenance and record the installed website
   file hashes. Do not silently replace the installed inactive daemon release.
2. Recheck public A, absent AAAA, CAA and host listeners/firewall. Record the
   existing nginx configuration, service enablement and firewall state for
   rollback. Stop on drift, listener conflicts or unexpected existing sites.
3. Install the reviewed web directory as root-owned, non-runtime-writable files
   under `/var/www/lightning-goats-static-staging`. Keep backups outside this root.
   Prepare root-managed `/var/lib/lightning-goats-acme/.well-known/acme-challenge`.
4. Select the operator-approved Let's Encrypt ACME account/contact and obtain
   explicit approval for issuance/account terms and public TCP80/TCP443 ingress.
   No production account key, DNS credential or OpenHAB token is needed on this
   VPS. The existing Let's Encrypt-only CAA proposal remains a separate DNS step.
5. Before the first certificate exists, enable only the candidate's HTTP server
   block. Validate `nginx -t`, then start/reload nginx using the reviewed host
   service mechanism. Permit only the approved IPv4 TCP80 ingress, preserving
   SSH and all other firewall rules. Verify an inert challenge file externally.
6. Use the approved ACME client's webroot mode with
   `/var/lib/lightning-goats-acme`, only `feeder.lightning-goats.com`, and Let's
   Encrypt. Do not use an automatic nginx installer or DNS plugin that can edit
   unrelated configuration. Record account choice, chain, expiry and renewal
   mechanism without disclosing private keys.
7. Once the certificate paths exist, enable the complete static candidate,
   validate `nginx -t`, permit the approved IPv4 TCP443 ingress and reload.
   Leave IPv6 ingress/listeners, daemon/gateway units and production DNS unchanged.
8. Verify trusted TLS without insecure flags from an independent client; record
   SNI/hostname, issuer, expiry, redirect, assets and disabled application paths.
   Verify renewal in the ACME test environment. Confirm no daemon or gateway was
   started and no production credentials were installed. Record nginx/firewall
   diffs and certificate/website provenance as static acceptance only.

## Rollback

Remove only the new site's enabled configuration and validate nginx before reload.
If nginx was previously inactive and this is its only new site, return it to its
recorded inactive state. Remove only firewall rules introduced by this stage,
using their recorded identities; do not flush tables or change SSH. Preserve
certificate and asset evidence privately for review. Do not alter production DNS,
the operator's staging A record, WireGuard or old-VPS availability during rollback.

## Context access exception

The temporary `lg-inspect` namespace uses the operator-reserved/registered
`10.8.0.12/32` identity. Alongside approved pinned SSH inspection, the operator
explicitly authorized TCP7543 to home `10.8.0.6` for Hexmem. Only that namespace's
output and established-reply rules were extended. Hexmem 2.0.0 health and MCP
context reads succeeded; credential-related and unrelated memory records are
excluded from subsequent scoped reads. Historical memories do not override the
current source contract. This temporary development exception is not F09 runtime
containment and must not become application authority or an application route.
