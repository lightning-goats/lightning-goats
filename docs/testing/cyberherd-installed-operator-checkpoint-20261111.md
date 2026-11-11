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

## Membership-control extension — qualification history

The same guarded test-only client now exercises actual HTTP activation and manual
score assignment when its synthetic gateway reports ready, stale-revision refusal,
ban/refused reactivation/explicit unban, and exact 21-sat score retention across a
real member restart. Manual scores must leave lifetime paid sats zero and the
configured zero member allocation unchanged. The actual public roster must show
points normalized from 210 weight units by the configured joining minimum, and
a zero share for the synthetic member.

After only the isolated gateway is paused again, a new score mutation must fail
while a committed score's original receipt remains retrievable. Removal, ban,
unban and manual reset must work while paused; reset advances the cycle once and
retains the ban until explicit unban. Another actual member restart must retain
the final inactive member/cycle/revision, and retries of both the previous-cycle
score and reset must return original receipts without restoring score or resetting
again. Existing template opt-out remains retained. The public roster ends empty.

The fixture retains its original unknown physical UUID throughout. The real core
must make no gateway POST and retain its original credit, threshold, due count,
remainder and unresolved UUID. The fixture has no public network, real provider,
real identity or physical destination. These new assertions await exact native CI
and installed systemd execution at the initial checkpoint; final qualification
is recorded below. Production operator routes and configuration remain unchanged.

The first extension source `bbd4cca32bbe268c36b907b8356a3c2e9ad0aab0`
passed native Rust and the other eight workflows but failed Deployment run
`42966027465`, systemd job `130345440657`, when the ready-command client exhausted
its retry loop on 409. That loop waited about three seconds while the actual
installed intake configuration polls every five seconds. The client now permits
three intake cycles under a monotonic 15-second deadline, retaining the exact
UUID/body/revision and fresh authorization on each attempt. Explicit refusal
tests still expect immediate 409, and the service's ordering fence and poll rate
are unchanged. The fixture pins its five-second configuration; subsequent exact
qualification remains required, rather than rerunning unchanged failed source.

Source `c66e52c2ca69d264f0ed13c595a80e0d9fa0dc19` also failed the ready
activation in Deployment `42967307933`, systemd job `130349375348`, after the
longer deadline. Persistent broken-pipe refusals revealed that the client was
retrying five times per second against a bridge limited to two admissions per
second (burst four); activation needs separate ordering and safety reads. The
client now backs off two seconds after conflict, paces distinct mutations, and
allows admission to refill before explicit refusal cases. Neither production
bridge capacity nor the ordering/paused-state checks were relaxed. This correction
still requires exact installed qualification.

Source `8d4c74cd9fb46e7a106661aa79c2a6eb77d00655` progressed through actual
activation and manual score assignment, then failed the ban command in Deployment
`42968691665`, job `130353829643`. Intake now records repeated explicit event-batch
refusals after the new roster state, so slower test retries are not a complete
resolution. The extension remains unqualified. Guarded test-only native diagnostics
now inspect the existing synthetic core export and member cursor on failure,
without altering the service's admission, export validation or ordering fences.
The diagnostic core opener requires the original single synthetic payment and
existing epoch, uses the exact current schema and applies no migration. These
diagnostics cannot execute without the existing protected disposable namespace
attestation and original file owners. Neither diagnostic is a production command.

Diagnostic source `db75a59ab59821b9d15e0d41bd003e7918cfc8c3` failed the public
score check in Deployment `42970946315`, job `130361180081`, after completing the
ready commands. Its native diagnostic accepted all eight core events through
sequence 8; the member cursor was 7. The data/export was valid. The public check
had waited only for one member and incorrectly expected 210 raw weight units as
displayed points. The public API normalizes points by the joining minimum. The
check now uses exact decimal normalization and the recorded final revision/cycle/
member/21-sat/zero-share projection, rather than a stale member count or raw units.

The bridge also rejected any admitted concurrent DB operation immediately. A
100-ms FIFO wait now allows synchronized intake/view/operator requests to take
turns, still within the existing eight-connection cap, two-per-second admission
budget and one DB-operation permit. Expiry still rejects; provider verification,
peer/role authority, source validation and financial fences are unchanged. Native
real-frame regressions hold the turn, prove an admitted request stays pending and
then returns its original batch after release, and prove timeout rejection leaves
no stranded permit and permits a later request. The positive test polls an already
readable frame without sleeps or cooperative-yield interference. Exact native CI
and installed acceptance were still pending at that corrective checkpoint.

## Final membership-control acceptance

Source `886e6438b83497f0ef4db9931620a21941c676a9` passes all ten workflows.
Deployment `42973865835`, installed systemd job `130370388400`, passes every
ready/paused/restart control above, exact normalized public score and zero share,
final empty roster, unchanged core credit/unknown UUID and zero gateway POSTs or
provider operations. Retained artifact `13834618387` has ZIP SHA256
`0060225aaad28da8f13acd16cc4784265af91dc80d3a4009a6f1badc9e0d5080`;
all six internal checksums and the exact named phase assertions were checked.
Artifact source `6830038b2dd2a650c84bf712a9959e59885733cc` independently
compares with zero file differences. Strict native CI also passes both real-frame
bridge regressions. This retains, rather than dismisses, the earlier failed runs.

The bounded core correction is subsequently
[deployed with unchanged paid images](cyberherd-bridge-turn-live-20261111.md).
Production operator auth/routes remain disabled pending the operator-selected
public-key allowlist. The synthetic fixture does not qualify an actual operator
browser, public production ingress, financial execution or recipient delivery.

## Optional authenticated edge transport — isolated checks

Three new assembled TLS/nginx tests pass locally with harmless backends: the
four exact optional routes preserve query, Authorization, Origin and command
bytes; wrong methods, oversize commands, unlisted paths and canary requests cannot
reach the backend; synthetic private fields/headers are absent from logs. Native
signature and operator authorization remain the separate installed exercise.

The installed nginx/ModSecurity connector and pinned CRS 4.30.0 also pass all
eleven isolated acceptance cases with the optional routes assembled, including
exact command/header transport, private logging and continuing CRS enforcement.
Only an unprivileged ephemeral loopback nginx with mocks was used. The actual
production nginx was neither changed nor reloaded, and these transport fixtures
do not claim production routing or native authorization accepted a request.

## Integrated native TLS edge — candidate

The guarded disposable systemd rehearsal now additionally assembles the shipped
HTTP limits and four optional operator locations in a separate loopback nginx.
An ephemeral CA signs its `example.com` server certificate. The test-only native
client trusts only the extra fixture CA alongside its normal roots, verifies the
server name, disables proxies/redirects and resolves that fixed name only to the
attested namespace's `127.0.0.1:443`; no public DNS or network is used.

Actual signed requests must traverse TLS/nginx and the native service together:
guest self-read, nonce replay and guest/operator/Origin/body refusals, an exact
signed template query, settings mutation with zero allocation, original receipt
retry and exact revision/settings readback. Requests respect the shipped rate
limits. Private authorization/body fields must stay out of nginx logs. The
existing native status comparison still verifies unchanged core credit, due count
and original unresolved UUID, with no gateway POST. Evidence remains private.

Local guard tests pass refusal of an ordinary user, the initial host namespace
even with CI labels, and a namespace with an external interface. Exact native CI
and installed execution of this integrated phase remain pending. Earlier separate
TLS mocks and direct native HTTP checks do not establish its result. Production
operator routes/configuration, actual user credentials and financial activation
remain unchanged. The integrated phase does not yet exercise a browser or WAF.

Initial integrated source `711dbef5270af2c7f2ae47d4a130c567e9d95111`
failed CI `42978879287`, Rust job `130384310178`, on `clippy::cmp_owned` in
the test client's settings readback. The assertion now compares directly with
the saved command's exact JSON value; it does not adopt the numeric-comparison
suggestion for a string-valued API field or weaken Clippy. Corrected source still
requires its own complete native and installed qualification.
