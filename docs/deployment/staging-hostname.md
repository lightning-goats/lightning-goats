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

## Historical public DNS observation

On 2026-09-11 all three authoritative ZoneEdit servers returned authoritative
NXDOMAIN for A, AAAA, CNAME and CAA queries at this name. Their SOA serial was
`1780000372`, with 300-second negative-cache TTL. This is twelve public queries,
not a complete zone export or an authenticated account inspection. Preserve the
[source-pinned record](../testing/evidence/staging-hostname-dns-20260911.json).

No existing service was found at this name in those answers. Recheck immediately
before any approved DNS operation; absence now does not authorize a future write.

## Current DNS and website evidence

At 2026-09-11 17:27 UTC all three delegated ZoneEdit authorities and public
resolvers 1.1.1.1/8.8.8.8 returned `64.177.40.118`, TTL 3600, for the staging A
record. Authoritative SOA serial is now `1789146394`. The operator published the
record; this session performed read-only queries. This supersedes the earlier
NXDOMAIN observation without deleting it. See the
[public query evidence](../testing/evidence/staging-dns-positive-20260911.json).

AAAA is absent. CAA is absent at both the staging hostname and zone apex; the
operator-approved Let's Encrypt-only CAA proposal remains unapplied by this
session. See `domain-readiness.md`. No IPv6 listener or DNS record is proposed for
the static acceptance stage.

The authoritative website was imported in draft PR #48. Its offline browser
checks passed with intercepted external traffic. Draft PR #49 adds the actual
owner adapter; draft PR #50 records the unresolved JDBC finality gap. None of
these establishes publicly trusted TLS or physical feeding acceptance.

## Remaining activation gates

Use [static staging acceptance](static-staging-acceptance.md) for the initial
website-only stage. It intentionally has no application upstream and forces
payments disabled. The broader canary example above remains a later integration
stage, requiring harmless owner/provider boundaries and separate review.

Public 80/443 activation and certificate issuance require an explicit operator
step after review. Preserve apex, www, mail, production services and the old VPS.
Do not overwrite the installed inactive runtime release's provenance. Final
account/secret review, F09 home-enforced containment, provider acceptance and
physical-owner finality remain open. Production remains HOLD.
