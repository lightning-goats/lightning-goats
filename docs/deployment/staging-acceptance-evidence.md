# Staging acceptance evidence and outstanding gates

Production remains HOLD. This record separates repository/mock evidence from
observations that need authorized host/source access. Keep #6/#15/#16 open and
all integration/remediation PRs draft pending review. Green CI does not authorize
real invoices/payments, physical feeding or cutover.

## Source and artifact gate

F11/F12 candidate: `a3b17e809ba956407a98c35a07685f3d61402050`, draft #37,
stacked on #36 through #31. The staging-evidence branch starts at that exact SHA.
Do not start from main: it does not contain the complete remediation stack.
Require the final candidate's Rust, Security and Deployment workflow results,
full SHA, BUILD-INFO and archive/binary SHA256SUMS. A later merge needs its own
integration checks; passing checks on an earlier stack head are not transferable.

Local environment: Fedora Server 44 x86_64, Rust 1.88, nginx 1.30.4, synthetic
credentials, temporary SQLite stores, loopback daemon/gateway/HTTP/WS mocks.
Tests have not contacted the physical owner or created a real provider invoice.

## Paired-store restore rehearsal

Stop all daemon, gateway, publication and other writers before capturing the pair.
Use SQLite online backup or `VACUUM INTO` after quiescing; copying only a database
file while WAL writers remain active is not a consistent backup. Keep the two
stores, their configuration/source revision and the evidence timestamp together.
Never start two physical owners or old and restored writers simultaneously.
A stale backup can omit physical activity after its capture. The mock rehearsal
keeps all writers stopped during that interval; it does not resolve lost physical
history. Before physical enablement after any real restore, reconcile the pair
against the actual owner under the separately approved F04 contract. Keep physical
action disabled while the history or owner outcome is uncertain.

The packaged helper accepts only a new private destination and does not start
services, copy credentials, change ownership/accounts, replace current databases
or change networking:

```sh
python3 deploy/scripts/restore-stores.py \
  --source-directory /path/to/quiesced-pair \
  --destination-directory /path/to/new-restore \
  --writers-stopped
```

The explicit flag is an operator attestation; the helper cannot prove remote
writers are stopped. Sources must contain `daemon.db` and `gateway.db` with the
final schema. It reads them through SQLite's read-only backup API, checks integrity,
foreign keys and required tables, rotates only the destination overlay stream
identity, and records destination SHA256 hashes in `RESTORE.json`. Files are 0600
inside a 0700 directory. Existing destinations are never overwritten. Any failure
leaves the destination unaccepted; never use a directory with `INCOMPLETE` or
without a valid manifest. Existing staged restores require a new destination for
another attempt. For another offline restore mechanism, run the ctl
`reset-overlay-stream` command before startup.

The Rust paired-store regression starts actual daemon and gateway binaries from
the shipped canary examples with a harmless loopback UUID-echo owner. It captures
an unresolved command, an issued signed BOLT11 fixture, a synthetic settlement,
and a failed pending Nostr outbox entry with locally signed synthetic bytes.
Both processes stop before snapshots. Restore rotates overlay identity while
preserving all financial/request/outbox state. Restart must retain credit and
poll the original UUID with exactly one total mock owner command; a late matching
acknowledgement causes exactly one debit and one confirmation event. Canary mode
must preserve the exact signed outbox text and retry metadata without publishing.
This proves the software restore boundary, not recovery against the live owner or
actual signer/relay. Attach execution results after the full checks finish.

## Network inventory and policy review dependency

The user supplied `.12` as a candidate staging identity. Local inventory found no
active WireGuard interface. A separate keypair/template was prepared outside Git
and syntax checked; it remains inactive. Address reservation and hub registration
have not been confirmed. The supplied broad route is not containment evidence.

Before choosing/activating an address, obtain authorized read-only inventories
from the old hub and home host. Record peer public keys/AllowedIPs and IPv4/IPv6
addresses/routes without private or preshared keys; all UFW/nftables/raw rules,
forwarding/NAT, established flows, alternate LAN links and relevant listeners.
Include where each rule executes and its order/default policy. Do not export
WireGuard dumps/configuration containing keys or service credentials.

| Path / identity | Required acceptance evidence |
| --- | --- |
| Inventoried staging peer to harmless gateway | Positive HTTP result over the approved gateway port, IPv4 and IPv6 |
| Staging/hub-attributable traffic to OpenHAB 8080, weather 5000, SSH 22, PostgreSQL 5432 | Negative connection results enforced at the home boundary |
| Staging/hub-attributable traffic to other LAN/peer destinations | Negative tests covering forwarding, NAT and alternate routes |
| Administrative source `.10` arriving through a broad hub peer | Source-attribution/spoofing test; IP alone is insufficient authentication |
| Independent operator administration | Authenticated management path and console recovery survive the candidate policy |
| Existing established flows and broad accepts | Demonstrate removal/containment effect; a new narrow allow is insufficient |

Known `.1` old hub, `.6` home host, `.10` laptop must retain their authorized
roles. The existing non-applying firewall worksheet stays non-applying. Actual
home policy cannot be reviewed completely without inventory; no household rule,
peer, DNS or route changes are authorized by these mock results.

## Remaining external acceptance

- F04: authorized read-only physical-owner source/items/rules and sanitized
  receipt/pending/rejected/completed fixtures. The legacy permissive parser is
  not an accepted live contract. Physical enablement remains blocked.
- Authoritative website source/assets and confirmed public contact identity:
  import and test payment UI, independent Nostr chat/auth, static routes, and
  version-1 overlay reconnect/reset/skip/expiry handling in a real browser.
- Clean installation of the final reviewed archive: root-owned binaries/config,
  separate non-admin runtime identities, final credential permissions/scopes,
  public TLS/IPv6, and final service ownership. Temporary mock startup is not a
  substitute for this gate.
- Authorized signer/relay behavior, live receive-only/P2P account compatibility,
  and operator resource sizing/history retention review.
- Separate explicit approvals for tiny real payment, controlled physical test,
  production DNS/WireGuard cutover and changes to old-VPS availability.

## Installed-release rehearsal continuation

A following candidate adds `install-rehearsal.md` and a reproducible isolated
installation test against the actual release archive. Root-owned binaries/config,
separate non-root processes, direct credential separation, migrations/startup,
six discovery routes and restart idempotency are checked inside a fresh loopback-only
network namespace. The fixture creates no host service/user or network policy.
Its evidence narrows the installation gap; final systemd encrypted credentials,
public TLS, actual identities/privilege revocation and live acceptance remain open.
