# Static staging activation, 2026-09-11

Production remains **HOLD**. The operator explicitly approved website activation,
IPv4 TCP80/443, Let's Encrypt issuance/account terms, and supplied the account
contact privately. The separately approved owner work is a repository-only
proposal; no physical-owner change was made.

## Result and source

`https://feeder.lightning-goats.com/` serves the static website from PR #51 commit
`d04fcffee9f3f6db4ba9ee21dbd59ecc8546833d`, tree
`c73235758ab32b75d7435c38b794286d4ba1ab0e`. The exact nginx site matches SHA256
`e541d1dc2e002c95957be1ec2b2bdb6c851633c91d4569ed71531c6b151338ea`.

Required checks passed: [Rust](https://github.com/lightning-goats/lightning-goats/actions/runs/34628402810),
[Security](https://github.com/lightning-goats/lightning-goats/actions/runs/34628402818),
[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/34628402847).
The tested merge was `a4291b335b8da4e9197f96a3e41063cebcdd7100`.

Package artifact 10275023181 has published ZIP SHA256
`3cf6440a10ba66dac3f7dab385cf27a0dec945e30863ccab63632ba0d8f03df3`;
systemd artifact 10275143140 has SHA256
`2557b90b48683def523b83bd37c97bb1a0cb12b8556f982ec8ce9a5411416dc8`.
Both expire September 25. The connector supplied a package download URL, but
this VPS received HTTP403; **the ZIP bytes were not verified locally or installed**.
Instead, static files were read from exact Git objects at the passing source
commit, hashed, and installed root-owned. No binary was installed and the prior
inactive daemon release provenance remains unchanged. The
[acceptance JSON](../testing/evidence/static-live-acceptance-20260911.json) records
this distinction, static-file hashes and public certificate metadata.

## Host changes

Fedora 44 nginx 1.30.4 was inactive/disabled. Its stock default HTTP server had
wildcard IPv4/IPv6 listeners. The original `nginx.conf`, firewall observations,
HTTP-only intermediate site and source manifest were saved under root-only
`/etc/nginx/lg-static-backup-20260911`. Only the stock default server block was
removed from `nginx.conf`; existing module, MIME, logging and conf.d includes
were retained. The reviewed site was installed in conf.d.

The web root is `/var/www/lightning-goats-static-staging`, root:root, directories
0755 and files 0644. ACME webroot is `/var/lib/lightning-goats-acme`. SELinux stays
Enforcing; the ACME subtree has a persistent `httpd_sys_content_t` file-context
mapping and restored labels. Website labels use the same content type.

The HTTP-only block first served an inert challenge. Certbot 5.7.0 was installed
from Fedora packages, registered the approved account and issued only the staging
hostname certificate using webroot mode. After issuance, the complete site passed
`nginx -t` and was reloaded. nginx is now enabled. No automatic nginx installer
or DNS plugin was used.

Only these firewalld rich rules were added, both runtime and permanent, in
`FedoraServer`; no firewall reload or flush was used:

```text
rule family="ipv4" destination address="64.177.40.118" port port="80" protocol="tcp" accept
rule family="ipv4" destination address="64.177.40.118" port port="443" protocol="tcp" accept
```

SSH and existing rules remain intact. Web listeners bind exactly the new public
IPv4 address; no IPv6 web listener exists. The existing forwarding policy was not
changed and is not claimed as final F09 containment. Canary daemon and gateway
remain inactive. No production DNS, home firewall, WireGuard or feeder change
was made during activation.

## Certificate and renewal

Trusted TLS 1.3 verification succeeded with hostname/SNI checking. Let's Encrypt
issuer YE2 certificate validity ends `2026-12-10T16:43:22Z`. The certificate's only
SAN is `feeder.lightning-goats.com`. Keys/account data remain root-managed and
are not committed. CAA is still absent at the staging name/apex; the operator's
Let's Encrypt-only CAA proposal remains a separate DNS action.

`certbot-renew.timer` is enabled. The root-owned executable deploy hook
`/etc/letsencrypt/renewal-hooks/deploy/50-lightning-goats-nginx` checks the exact
renewed lineage, runs `nginx -t`, then reloads nginx. Its content is:

```sh
#!/bin/sh
set -eu
[ "${RENEWED_LINEAGE:-}" = /etc/letsencrypt/live/feeder.lightning-goats.com ] || exit 0
/usr/sbin/nginx -t
/usr/bin/systemctl reload nginx
```

`certbot renew --cert-name feeder.lightning-goats.com --dry-run --run-deploy-hooks --non-interactive` completed successfully, including the nginx validation/reload hook. Certbot labeled nginx's ordinary successful stderr output as hook error output; the renewal command exited zero and reported all simulated renewals succeeded.
Certbot deliberately adds a random delay to noninteractive renewal, so a quiet
log during that interval is not a validation failure. See the
[Certbot renewal documentation](https://eff-certbot.readthedocs.io/en/stable/using.html#renewing-certificates).

## Acceptance scope

Requests from the VPS and the independent home host succeeded with normal public
DNS and certificate verification. The homepage hash matches the pinned Git blob.
The server-generated `site-config.js` forces payments false; it intentionally has
a different hash from the on-disk default config. LNURL discovery/callback,
status, overlay and OpenHAB paths return 404; webhook POST returns 405.

This is live static HTTPS evidence, separate from the 46 passing local deployment
tests and earlier intercepted desktop/mobile browser checks. No live Nostr
publication, payment, provider subscription, owner actuation or dispensing test
was performed. External web-tool retrieval was unavailable; the pinned home-host
check supplied the independent network observation. Full payment/feeder staging
acceptance remains open, including F04 finality and F09 runtime containment.

## Rollback from this observed state

Stop and disable nginx, remove only this site's conf.d entry, and restore the
saved stock nginx.conf while nginx remains stopped. Remove each of the two exact
rich rules above from runtime and permanent firewalld state; preserve SSH and do
not flush tables. Preserve the backup and certificate evidence. If retiring this
staging certificate, disable its renewal schedule/hook only after checking that
no other certificates have since been added. Leave production and staging DNS,
WireGuard, the physical owner and old VPS unchanged. Asset/SELinux cleanup is
optional after evidence retention and must target only the recorded new paths.
