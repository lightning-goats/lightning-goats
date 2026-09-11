# Domain and DNS readiness

Production remains **HOLD**. This is read-only evidence for #19 and the #15/#16
acceptance gates. It does not authorize DNS changes, certificate issuance, account
changes or cutover. The old VPS remains the public payment-routing destination.

## Public baseline, 2026-09-11

Subsequent staging-name selection: the operator approved
`feeder.lightning-goats.com`. See `staging-hostname.md` for its authoritative
NXDOMAIN baseline, aligned application/nginx examples and unapplied DNS proposal.

The continuation starts from draft #42 head
`361e28e1b6c7bee724c7e462f611bf841591fc41`. Its Rust, Security and Deployment
workflows passed; application/source checks do not establish domain readiness.

[Sanitized DNS, registry and TLS evidence](../testing/evidence/public-dns-baseline-20260911.json)
records 40 bounded public DNS queries: all three authoritative ZoneEdit servers
for the apex and `www`, plus delegation/DS queries to two `.com` authorities.
All 36 child-authoritative answers agree, including SOA serial `1780000372`.
The full raw DNS response record is retained privately with SHA256
`a37714a629ac2c1143ef9e10a05a5261135c3f2f26534023f5d1a8a1be8125c6`.

| Record/control | Observed state | Acceptance implication |
| --- | --- | --- |
| Apex A | `45.76.234.192`, TTL 3600 seconds | Current old-VPS rollback value; recheck immediately before any approved change |
| `www` | CNAME `lightning-goats.com.`, TTL 3600 seconds | Follows the apex; preserve alias behavior unless separately reviewed |
| Apex AAAA | Authoritative NOERROR with no answer | No public apex IPv6 destination observed; new-VPS IPv6 still needs explicit acceptance |
| Child NS | `dns1.zoneedit.com.`, `dns2.zoneedit.com.`, `dns3.zoneedit.com.`, TTL 3600 | All three agree; parent delegates the same set with TTL 172800 |
| Apex MX | Priority 10 `mail.anonaddy.me.`, priority 20 `mail2.anonaddy.me.`, TTL 3600 | Preserve mail service during the website/payment migration |
| Apex TXT | Two matching records on all authorities | Values remain in the operator-controlled inventory; full-zone purpose/ownership review is still needed |
| Apex CAA | Authoritative NOERROR with no answer | Intended certificate authorities and subdomain usage need review before adding policy |
| DNSSEC | No child DNSKEY or parent DS answer; registry `delegationSigned: false` | Delegation is reported unsigned; no DNSSEC acceptance claimed |
| Registry locks | `client transfer prohibited`, `client update prohibited` | Public registry lock evidence only; does not prove DNS-account protections, MFA or recovery |
| Existing-site TLS | Apex/`www` hostname and system trust validation passed, TLS 1.3; Let's Encrypt YE1, expires 2026-10-07 10:45:58 UTC | Existing old-VPS certificate observation only; no new-VPS certificate or renewal acceptance |

Registry metadata comes from the public
[Verisign RDAP domain endpoint](https://rdap.verisign.com/com/v1/domain/lightning-goats.com).
No personal contact data was retained. The registry reports expiration at
`2027-06-26T20:18:30Z`; this does not prove renewal/payment/account recovery setup.

The DNS observations are not a complete zone export. They cannot enumerate
unknown subdomains, wildcard records, delegated zones, service records or
account-only settings. No zone transfer or authenticated DNS access was attempted.
Parent denial records were retained, but no independent DNSSEC signature-chain
validation was performed. TLS was a handshake only, with no HTTP invoice request.
Unrelated certificate names and public TXT values are omitted from public evidence.

Recheck with `dig` against each server/name/type listed in the JSON. For example:

```sh
dig @dns1.zoneedit.com lightning-goats.com A +norecurse +dnssec +time=3 +tries=1 +noall +answer +authority +comments
dig @a.gtld-servers.net lightning-goats.com DS +norecurse +dnssec +time=3 +tries=1 +noall +answer +authority +comments
```

Keep the answer/authority sections and response flags so an empty answer can be
distinguished from a timeout, refusal or non-authoritative response.

## Operator-dependent controls

- The operator confirmed control of both registrar and ZoneEdit accounts on
  2026-09-11, with hardware-key MFA configured and account recovery tested for
  both. This is operator attestation, not an authenticated account inspection.
  Review any additional authorized administrators during the account inventory. Never put
  passwords, recovery codes, API tokens or private keys in this repository.
- Obtain a complete authorized zone inventory and classify records by service.
  Preserve mail/TXT and any unrelated active services. Identify stale records
  before proposing removals; public apex queries cannot establish that they exist.
- The operator approved Let's Encrypt as the sole CA on 2026-09-11. Reconcile
  that decision with the complete zone inventory before applying CAA policy.
- Confirm DNSSEC eligibility and the exact provider/registrar procedure before
  preparing an activation plan. ZoneEdit's published procedure requires the
  domain to be registered through its service and changes authoritative servers
  during enable/disable. Do not treat this as a local record-only operation.
- If a control is unsupported, record the provider evidence and operator rationale
  for `N/A`; missing access or an unanswered decision is not `N/A` or acceptance.

### Unapplied CAA proposal

Proposed apex record, using the observed 3600-second TTL:

```dns
lightning-goats.com. 3600 IN CAA 0 issue "letsencrypt.org"
```

This selects the operator-approved CA. Without a separate `issuewild` policy,
`issue` also governs wildcard issuance. This proposal adds no account or ACME
validation-method restriction. According to
[Let's Encrypt's CAA documentation](https://letsencrypt.org/docs/caa/), records
are additive, closer subdomain records override inherited policy, and lookup
follows CNAMEs. A new apex record alone cannot prove a domain-wide restriction.

Before an explicitly directed DNS change, export the complete zone and review
existing CAA RRsets, subdomain overrides, aliases and delegated names. Do not
append the proposal alongside another CA authorization and call it exclusive;
prepare any required replacements separately. Keep `www` as its existing CNAME,
without adding a conflicting CAA record at that name. Preserve unrelated records
and save the original RRsets for rollback. After any authorized change, query
every authoritative server and relevant name and check existing renewal paths.
No CAA record has been applied and no certificate has been requested here.

Provider references checked on 2026-09-11:
[ZoneEdit CAA management](https://support.zoneedit.com/en/knowledgebase/article/managing-caa-records),
[ZoneEdit DNSSEC procedure](https://support.zoneedit.com/en/knowledgebase/article/dnssec),
and [zone inventory utilities](https://support.zoneedit.com/en/knowledgebase/article/bulk-editing-and-information-retrieval).
Provider documentation does not establish this account's actual capabilities.

## Cutover preparation and rollback constraints

Keep the captured values as a baseline, not a command to restore blindly. Requery
all authorities and compare against the operator-approved complete zone export
before an eventual change. If the source records have advanced, review the newer
state and preserve both observations.

The current apex/`www` positive TTL is 3600 seconds. An approved TTL reduction
must be published and the previous cache lifetime allowed for before relying on
it; changing TTL at the cutover does not expire older cached answers. Do not lower
TTLs or add records during this HOLD. The current authoritative negative SOA TTL
is 300 seconds; record absence and delegation caches also matter when introducing
new names or changing DNSSEC. DNS propagation is not an atomic switch.
See [RFC 1034 caching](https://www.rfc-editor.org/rfc/rfc1034) and
[RFC 2308 negative caching](https://www.rfc-editor.org/rfc/rfc2308).

Prepare the final hostname/SNI, A/AAAA decisions, trusted certificate/renewal and
complete nginx/static-site behavior on the new VPS first. Retain the old VPS and
its working certificate/renewal dependencies until the approved rollback and
observation gates are satisfied. Public TLS here does not prove the old website
source has been imported, its clients work, or that shared legacy services can be
retired safely.

DNSSEC/CAA policy changes and payment-routing changes each need a concrete review
and explicit operator-directed step. Keep F04 owner binding, F09 containment,
website/browser, signer, credentials/privileges and all live tests open alongside
these domain controls. No public observation lifts production HOLD.
