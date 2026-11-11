# Installed operator HTTP qualification — candidate, 2026-11-11

The disposable installed systemd rehearsal now includes an optional operator
API stage. This is source awaiting exact CI qualification; it has not run on
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
