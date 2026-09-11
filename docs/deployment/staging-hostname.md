# Approved staging hostname

The operator selected `feeder.lightning-goats.com` on 2026-09-11. This is the
temporary public staging hostname for the new VPS. Production remains HOLD.

## Prepared binding

The shipped canary daemon and nginx examples now agree on:

| Setting | Prepared value |
| --- | --- |
| Canary LNURL public base URL | `https://feeder.lightning-goats.com/` |
| nginx HTTP/HTTPS server name | `feeder.lightning-goats.com` |
| HTTP redirect target | `https://feeder.lightning-goats.com$request_uri` |
| Certificate chain | `/etc/letsencrypt/live/feeder.lightning-goats.com/fullchain.pem` |
| Certificate key | `/etc/letsencrypt/live/feeder.lightning-goats.com/privkey.pem` |
| Static root | `/var/www/lightning-goats-canary` |
| Canary upstream | `127.0.0.1:8788` |

The private certificate key path is a reference only: no certificate was issued
or key installed. The canary example remains sandbox/harmless-owner-only and does
not enable Nostr publication. The hostname is for the public website/application
edge, not the trusted physical-feeder gateway or generic OpenHAB APIs.

## Public DNS observation

On 2026-09-11 all three authoritative ZoneEdit servers returned authoritative
NXDOMAIN for A, AAAA, CNAME and CAA queries at this name. Their SOA serial was
`1780000372`, with 300-second negative-cache TTL. This is twelve public queries,
not a complete zone export or an authenticated account inspection. Preserve the
[source-pinned record](../testing/evidence/staging-hostname-dns-20260911.json).

No existing service was found at this name in those answers. Recheck immediately
before any approved DNS operation; absence now does not authorize a future write.

## Remaining activation gates

The concrete IPv4 DNS proposal is:

```dns
feeder.lightning-goats.com. 300 IN A 64.177.40.118
```

This record is **unapplied**. Hostname selection does not approve DNS changes or
certificate issuance. Full-zone review and an explicitly directed DNS/TLS step
remain necessary. IPv6 is not proposed for DNS until the external path and its
containment are accepted; the shipped generic dual-stack listener is not proof
of IPv6 readiness. Preserve the existing apex, `www`, mail and unrelated records.
The operator-approved CA is Let's Encrypt; see `domain-readiness.md` for the
unapplied CAA proposal and account-control decisions.

The persistent inactive installation still contains its original archived
example, not this newly bound one. Do not silently overwrite its source/hash
record or activate it: install a reviewed configuration when credentials, the
harmless owner gateway, website source and network boundaries are ready.

Authoritative website source import and browser acceptance remain open. An nginx
mock regression proves routing/redirect behavior with temporary local TLS; it
does not prove publicly trusted staging TLS, DNS resolution, production-equivalent
browser behavior, provider access or physical feeding. Direct IP/SNI tests can be
used before DNS changes, using a separately reviewed test certificate and client
trust setup. Do not disable TLS verification for final acceptance.
