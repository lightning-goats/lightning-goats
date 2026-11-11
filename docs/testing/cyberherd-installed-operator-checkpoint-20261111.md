# Installed operator HTTP qualification — candidate, 2026-11-11

The disposable installed systemd rehearsal now includes an optional operator
API stage. The actual installed exercise passes on source
`1240974d798e74503cd20cca6d65dc9e1998875f` in Deployment run `42958315166`.
Its retained artifact `13828397449` has ZIP SHA256
`591fe4c890c3846934e46dd2dfb6d2b2033f9d47549b7dbcb65c4c62a53eae7d`;
all six internal checksum entries and the exact named operator proof flags pass.
Full source CI independently exposed a [feed-intent contention failure](feed-intent-contention-20261111.md).
The corrected source `55ed77b8a3f0c7cb50b031536cb53161de6d5e89` subsequently
passed all ten workflows, including full Rust and installed Deployment
`42960189058`. The repeated installed operator proof passes in artifact
`13829515806`, ZIP SHA256
`c41e423643f7560607f27d01615669dd5654357e7069ee28b82b1d6a7786c4db`;
all six internal checksums and named proof flags were independently checked.
Artifact source `dd7c4a28ce799a1e9da797843eae16b5165a2017` has an identical
tree to the branch source. This fixture has not run on
the production VPS and does not enable actual operator routes or public flags.
The [operator contract](../architecture/cyberherd-operator-interface.md) and
[full twelve-part plan](../planning/cyberherd-implementation-plan.md) are unchanged.

After the existing native schema/contact exercises, the guarded CI harness stops
only its synthetic member, adds a canonical HTTPS auth origin and one synthetic
operator public key, and resumes the actual installed native service under its
normal non-admin identity. Networking remains in the attested loopback-only
namespace, all credentials remain synthetic, override is confirmed ON and both
outgoing units remain masked. No production key, database or endpoint is used.

`TEST-ONLY-installed-operator-client` is a separate example, outside production
packages. It requires non-root execution, absence of credentials, the protected
root CI namespace attestation and a private owned fixture-state directory before
any network or file operation. It accepts no URL, key or general output path.
Its only network destination is the fixed local member listener. NIP-98 events
are signed with known synthetic keys using native BIP-340 and exact canonical
event hashing; they go only in HTTP headers, never to relays.

The first phase exercises actual HTTP handlers and private core ordering reads:
signed guest self-read; nonce replay, guest operator read/mutation, wrong Origin,
changed-body and paused-activation refusals; operator settings mutation with zero
allocation, template category opt-out, exact state readback and committed-command
retry after the revision has advanced. Forged Host/forwarded Host headers cannot
replace the canonical configured auth origin. All responses must retain no-store.
The roster must remain empty after every refused activation.

The harness actually stops and starts the member once and checks that its PID
changes. The second phase checks retained settings/template state, exact original
command receipt under fresh authorization, refusal of changed intent under the
same UUID and no additional revision change. Its original accepted authorization
is cryptographically rechecked and must still be no more than 30 seconds old
before testing replay, so ordinary 60-second expiry cannot substitute for durable
nonce rejection. Synthetic HTTP proof bytes remain private; only explicit named
positive flags enter the retained report. The no-actuation gateway must record
zero POSTs throughout.

This qualifies neither a real browser extension/household signer nor public
nginx/WAF operator ingress. Browser contract tests remain separate. It does not
qualify an unpaused activation/score mutation, provider payout, actual recipient
delivery or recovery adoption. Production operator/auth/publication/payout gates
remain closed until their separate qualified activation.
