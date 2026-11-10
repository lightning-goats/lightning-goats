# Installed native recipient/DNS checks — 2026-11-10

Qualified source `6582015a7eab9dd6deaa83cd77a9a03d133ddc26` and tested merge
`2ac875f5234a3f242f7ff89c0b397f81c3a45359` share tree
`705b0b5ebe15e43743473d7cd886c9acac95e2fc`. The
[read-only native helper](../deployment/cyberherd-recipient-qualification.md)
passed actual installed checks under the payout identity. No production program,
configuration, journal, DNS, firewall, payment or feeding was changed.

[CI](https://github.com/lightning-goats/lightning-goats/actions/runs/42860336465),
job `129984448136`, passed format, strict locked all-target/all-feature Clippy,
**496 tests, zero failures, four ignored**, and all four separate required root
fixtures. Root repeats and two separate initialization-example tests are excluded
from 496. [Security](https://github.com/lightning-goats/lightning-goats/actions/runs/42860336491)
and [synthetic real NIP-46](https://github.com/lightning-goats/lightning-goats/actions/runs/42860337674)
passed. These synthetic signing results do not establish household authorization.

[Deployment](https://github.com/lightning-goats/lightning-goats/actions/runs/42860336845)
passed all five jobs. Package job `129984449380` explicitly ran the new example's
test: only the native DNS-admission refusal can pass, while NXDOMAIN, connection,
TLS or timing failures cannot substitute. An earlier version failed rustfmt; the
reported formatting was corrected. An earlier separate nak download failed with
GitHub HTTP 500; subsequent exact-source download/acceptance passed without
weakening its pinned digest. The final source also verifies each exact DNS answer
set before testing refusal.

Test-only artifact `13784193195`, ZIP SHA256
`7c0aa76d145749d63d606e61ef7e1b331459fcf441aa4676161abf5678091f8f`,
was downloaded and its outer digest, internal checksums and tested source verified.
Only the read-only helper was extracted as an executable. Its SHA256 is
`4531b20efd7e6d8a72b4f69213e38d251e7297e91afb317ff38f76ddfbd96561`.
Installed program, hosts-fixture and assembled-unit hashes matched before either
unit started. Assembled systemd validation passed.

The first actual one-shot unit used a root-owned fixture hosts file bound over
`/etc/hosts` only in that unit's mount namespace. Native lookup returned the exact
private IPv4, private IPv6 and mixed public/private sets; all three native fetches
refused those answers before HTTP-client creation. No request to a trusted/private
network endpoint was attempted. The fixture entries remained absent from the
host's actual hosts file.

The second unit used actual public DNS and normal HTTPS certificate verification
to fetch `herd@lightning-goats.com` discovery metadata. Typed recipient validation
passed, advertising 1,000–1,000,000,000 msat bounds and no Nostr receipt support.
That is the current disabled native-zap configuration, not an invoice/payment
failure. No callback, invoice, provider or credential operation occurred.

Both units used the configured actual payout identity with private-network,
filesystem, capability and memory restrictions and no systemd credentials.
All three original role-journal hashes, all four configuration hashes and original
service-state records matched before/after. The existing receive process remained
unchanged; the CyberHerd units stayed masked. Health and `/api/v1/status` returned
200 after cleanup. Only the exact two temporary units were removed. The accepted
sanitized receipt, source identity, unit copies and reports remain root-private.

This qualifies the stated native discovery/resolver path, one selected public
recipient and controlled private/mixed answers. It does not qualify all recipients,
global firewall/forwarding, external public IPv6, invoices/transfers, household
signing, browser presentation, recovery adoption or financial activation. No new
application worker or monetary limit is enabled.
