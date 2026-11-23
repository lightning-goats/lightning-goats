# Breez reserve and Cashu mint deployment

This handoff covers stages 1 and 2 of the [wallet and mint plan](../planning/breez-spark-wallet-and-mint.md). HOME owns the reserve, issuer, and private WireGuard origin at `10.8.0.6`. The VPS agent owns public ingress at **`https://mint.lightning-goats.com`**. Strike remains the Lightning Goats payment path under the production HOLD.

## Source and service identities

| Component | Pinned source | Runtime identity | Persistent state |
| --- | --- | --- | --- |
| Reserve | Breez Spark `0.28.6`, `5bb1ca5229cd4d6d4e8cf66dfe9d01224db6cd2d` | `lg-reserve` | `/var/lib/lightning-goats-reserve-mainnet` with SDK storage and `reserve.sqlite3` |
| Issuer | Nutshell `0.22.1`, `3ab0137b0c6b214cecc2f883be606a668ae74e60` | `lg-mint` | `/var/lib/lightning-goats-mint-mainnet` with the SQLite mint database |

The [standalone package](../../deploy/breez-reserve) builds against both upstream lockfiles. The Spark patch uses the SDK-owned wallet for the lower-level invoice and receive lookup calls. The Nutshell patch uses the ordinary quote, paid-status, NUT-20, signing, and proof accounting code. Issuance commits signatures, the quote state and the liability in one transaction. The Cashu seed and reserve seed are independent. Mainnet and Regtest use distinct state and socket paths.

## Home installation and key entry

Build from a reviewed commit on the home host with the Rust toolchain selected by the pinned SDK, `protoc`, Poetry, and a relocatable Python 3.11 distribution. For example, use the Python executable from a managed `uv python install 3.11` installation.

```sh
deploy/breez-reserve/build.sh "$PWD/target/reserve-package" /path/to/managed/python3.11
sudo deploy/breez-reserve/install-home.sh "$PWD/target/reserve-package/bundle" "$(git rev-parse HEAD)"
```

Install the existing Breez Spark application key from the home terminal. The script prompts with echo disabled and writes a host-bound systemd encrypted credential. It refuses an existing destination so rotation is an explicit maintenance operation.

```sh
sudo /opt/lightning-goats-reserve/add-breez-api-key.sh
```

The root-only file is `/etc/credstore.encrypted/lightning-goats-reserve/breez-api-key.cred`. The mainnet reserve unit loads it as `breez-api-key`. If the package has yet to be installed, the [source copy of the key script](../../deploy/breez-reserve/add-breez-api-key.sh) writes the same credential. Keep the key in the operator's password manager for recovery on a replacement host.

Choose an encrypted offline recovery destination and run `sudo /opt/lightning-goats-reserve/provision-home-credentials.sh /absolute/new/recovery.tar.gpg`. It generates the two seeds and a separate project-quote token, checks the portable archive with the prompted recovery passphrase, and installs host-bound encrypted service credentials. Store the archive and passphrase through separate private channels. Activate the reserve locally with `sudo systemctl start lightning-goats-reserve@mainnet`. A local `GET /health` over `/run/lightning-goats-reserve/mainnet/receive/http.sock` reports the public reserve identity. The mint config pins that identity using `LG_RESERVE_WALLET`. Copy `/etc/lightning-goats-reserve/mint-mainnet.env.example` to `mint-mainnet.env`, set that identity, and start `lightning-goats-mint@mainnet`. The unit definitions support starting and restarting without host reboot. Use `systemctl status` and local capability requests to distinguish process startup from wallet sync. The mint startup check requires a fresh synced reserve balance.

The installed `accept.py` verifies wallet identity, network, amount, expiry, hash, and signed memo or digest. Run it with the bundle's Python interpreter and `PYTHONPATH=/opt/lightning-goats-reserve/current/site-packages`. `accept.py --help` shows Unix socket and private mTLS options. The receive probe also verifies the funding balance route returns HTTP 404 on that socket. The funding socket is restricted to the issuer's Unix group. Only that socket supports fee quotation, payment or outgoing lookup.

## Private contract

The wallet accepts one UUID per operation. `POST /v1/invoices` takes `operation_id`, `amount_sat`, `expiry_secs`, and a `description` object. Use `{"purpose":"mint_quote","memo":"..."}` for an ordinary quote, or `{"purpose":"project_quote","description_hash":"...","claim_pubkey":"..."}` for an exact 32-byte lower-case hex digest and a compressed claim key. Amounts are positive whole sats and expiry is 60 through 3,600 seconds. Response fields include signed `invoice`, `payment_hash`, `wallet`, `network`, `amount_sat`, `expires_at`, and the provider `lookup_reference`. `GET /v1/invoices/{payment_hash}` returns `pending` or a verified `settled` receipt. Identical create replay returns the stored BOLT11 even after expiry. Conflicting intent returns HTTP 409. An interrupted create without a durable provider response holds the UUID for operator review and returns HTTP 503.

The funding socket adds `GET /v1/funding/balance`, `POST /v1/funding/fee`, `POST /v1/funding/payments`, and `GET /v1/funding/payments/{operation_id}`. The fee request contains `invoice`. Payment adds `operation_id` and `max_fee_sat`. The SDK starts only the Lightning route with the prepared fee limit. Verified outgoing `settled` supplies principal, actual fee and preimage; a definitive correlated failure returns `failed`; ambiguity stays `pending`. One payment hash is admitted for at most one reserve send. Tests use fixture sends. Keep `LG_RESERVE_SPEND=no` and `MINT_BOLT11_DISABLE_MELT=TRUE` during the funded receipt-only test so a wallet cannot reserve proofs for a disabled route. The public mint needs authorized redemption before general availability.

`POST /private/v1/project-quotes` accepts the private bearer credential and `request_id`, `amount_sat`, `description_hash`, and `claim_pubkey`. The stable quote ID is `goats-` followed by the request UUID. The reserve operation binds the digest and claim key before contacting Breez. The response uses the ordinary Nutshell mint quote model. The collection client must persist the request, NUT-20 signing key and planned outputs before sharing the BOLT11. Use normal paid-quote status and `POST /v1/mint/bolt11` with the NUT-20 output signature. The issuer rejects internal melts into a project quote so paid status comes from the Lightning reserve. After a lost claim response, use the original blinded outputs with `/v1/restore` and the original blinding factors to recover the proofs. Ordinary public quotes use NUT-04 with no private project parameters. [NUT-20 specification](https://cashubtc.github.io/nuts/20/)

## VPS agent handoff for `mint.lightning-goats.com`

1. Coordinate the actual VPS WireGuard source IP with HOME through the existing handoff. The HOME inventory confirmed `wg0` at `10.8.0.6/32` and selected unused ports `8792` for public-protocol upstream and `8793` for an optional private receive/project upstream. HOME must confirm the specific VPS source before adding a narrow `wg0` firewall rule for either port. Never rely on the route prefix as a peer fingerprint.
2. Generate the VPS upstream client private key on the VPS and send HOME its CSR through the approved exchange. Use an edge-only client CA, separate from the private receive/project client CA. HOME returns the signed edge client certificate and the CA chain for `mint-home.internal`. Use `/etc/nginx/goats-mint/public-edge-client.pem`, `/etc/nginx/goats-mint/public-edge-client.key`, and `/etc/nginx/goats-mint/home-ca.pem` on the VPS, or update the example to match the private paths. The home certificate SAN must include `mint-home.internal`.
3. Install [VPS nginx template](../../deploy/breez-reserve/vps-edge.nginx.conf.example) in the VPS `http` context and render [the public route include](../../deploy/breez-reserve/public-routes.conf) to `/etc/nginx/goats-mint/public-routes.conf`, replacing `GOATS_MINT_SCHEME` with `https`. HOME uses the same route include with `http` over its Unix socket and the [home origin template](../../deploy/breez-reserve/home-edge.nginx.conf.example). Keep public requests on `8792`. Do not route public requests to `8793` or forward an authorization header from the internet. The public template exposes only the Cashu routes including the NUT-17 websocket. It limits body size and request rate. Apply the VPS site WAF for this hostname and test JSON POST and websocket upgrade with the full candidate ruleset. Obtain the public DNS record and TLS certificate under the VPS owner's authorization before reload and outside-tunnel acceptance.
4. Test `GET /v1/info`, `/v1/keys`, `/v1/keysets`, quote methods, websocket upgrade and out-of-policy amounts. Confirm rejection of `/private/v1/project-quotes`, `/wallet/health`, `/v1/funding/balance`, `/docs`, and generic OpenHAB routes through the public hostname. Check TLS failure when the upstream client certificate is absent, and confirm HOME uses the scoped source rule. Record the public URL, observed keysets and NUT capabilities for HOME.

## Acceptance, solvency and restore

The operator authorized up to **1,000 sats total into the test mint**, to stay as Cashu for Minibits. After the private issuer and canonical HTTPS route pass their checks, add `https://mint.lightning-goats.com` to the operator's Minibits wallet. Create the viewer top-up in Minibits and pay from an independent Lightning wallet, or send a separately held Cashu test token to Minibits and verify it can swap there. Keep the combined viewer and any collector test funding within the 1,000-sat total. Do not run a live Lightning melt under this test authorization. A later outbound qualification needs its own bounded test and fee scope. The SDK can issue signed BOLT11 invoices on Regtest. Its Regtest network does not exercise real Lightning settlement. [Breez test environments](https://sdk-doc-spark.breez.technology/guide/testing.html)

Use the local `solvency.py` helper with the sat mint SQLite path, the funding socket and the expected wallet identity. It reports the last synced reserve balance, issued liability, paid unissued quotes, other quote exposure, unsettled melt principal and fee reserve, issuer fees and settled Lightning fees. The two stores are read independently. In routine monitoring, compare successive samples and investigate pending transfers or a stale sync before declaring a deficit. The mint's configured watchdog independently tracks its backing. Quote limits and edge throttles are mandatory on this candidate. The example uses staging limits. The Nutshell balance cap checks issued liability at quote creation. Monitor paid unissued quotes separately before expanding rollout.

For a consistent snapshot, pause ingress, quiesce both processes, and copy the complete paired state directories, including any SQLite WAL files. Encrypt the snapshot outside the live directories and record snapshot time, binary/source revisions, network and reserve identity. Retain the portable encrypted seed recovery separately. On an isolated restore, restore both state directories with the matching credentials, keep ingress disconnected and ensure only one SDK owner can connect for that seed. Confirm the reserve identity and mint keyset, check a previously spent proof is still spent, redeem a known unused proof in the disposable qualification instance, and reconcile a pending quote to exactly one result. A stale database paired with a live signing seed can accept proofs already spent since the snapshot. Do not replace the active issuer database with stale state. If state continuity cannot be established, keep that signing key closed to issuer traffic and reconcile the incident before serving it. For an ordinary issue pause, control new issuance with `MINT_BOLT11_DISABLE_MINT=true` and preserve claim and melt routes for existing holders.

Build and process tests in the package use disposable keys. `tests/mint_acceptance.py` exercises actual Nutshell against a fixture reserve, covering normal issuance, swap, signed-zap digest, project claim ownership, lost-response restore, spent-state persistence and bounded melt reconciliation. Record mainnet Lightning settlement, DNS/TLS/WAF, independent Minibits access and a fully funded restore as open until each has been observed on the corresponding host.

## Qualification status

The standalone build uses Rust 1.95 and a locked Python 3.11 environment. Local tests covered the exact hash on signed Breez Regtest invoices, duplicate replay and pending status. The pinned Nutshell process passed the fixture flow for issuance, swap, NUT-20 owner enforcement, the original restore outputs, and one pending melt that reconciled with change after restart. Separate loopback nginx instances with disposable certificates passed both route syntax checks, the positive mTLS path and the public/private route exclusions. This did not establish a live VPS route.

The active reserve dependency audit retains [RUSTSEC-2026-0387](https://rustsec.org/advisories/RUSTSEC-2026-0387.html) in `crypto-bigint` `0.5.5` through the pinned Spark signing stack. The patched version is outside that dependency series. Resolve or obtain a reviewed disposition for the signing dependency before holding live funds. The candidate includes patched HTTP and SQLite crates plus the updated Nutshell runtime lock. The existing Lightning Goats application passed its own pinned Rust gates and separate advisory check.

### HOME installation record · 2026-11-19

- Installed candidate `537fb54e5db078948cbbd1bb9fa9f40730a3f8ab` at `/opt/lightning-goats-reserve/current`. The root-owned reserve binary has SHA-256 `ae8c98c0072b8e4a90f756ab7f2a0c43f4dedbff15ca0d5bf1b783005482264a`. The exact bundled Python site packages passed `pip-audit`.
- Installed Regtest units passed sync, authorized socket access, rejection of an unrelated local user, issuer denial of reserve database access, signed mint quote creation, and restart with the original reserve identity, keyset and invoice. The solvency probe reported a zero reserve and zero issued liability for this disposable instance.
- The supplied encrypted Breez application key passed an isolated mainnet probe with a disposable seed. Breez issued a signed 8-sat invoice. Replay returned the identical BOLT11 and status stayed `pending`. The probe ended with zero sats paid.
- The installed mainnet units are inactive. The remaining gates are a funded-reserve disposition for the Spark signing advisory, portable recovery for the actual reserve and issuer seeds, the VPS mTLS and public HTTPS handoff, and funded Lightning receipt and Minibits acceptance within the 1,000-sat authorization.

### HOME bounded test and VPS handoff · 2026-11-20

- The operator approved proceeding with the receipt-only test after review of the signing advisory. The dependency upgrade remains open for general availability. HOME's mainnet reserve drop-in at `/etc/systemd/system/lightning-goats-reserve@mainnet.service.d/cpu.conf` sets both `AllowedCPUs=16-27` and `CPUAffinity=16-27`. Each selected CPU was online and listed only itself in `topology/thread_siblings_list`. A transient service running as `lg-reserve` verified the effective mask. Recheck the topology after hardware or firmware changes and check the running reserve's effective mask at activation.
- HOME staged the WireGuard origin configuration at `/etc/nginx/goats-mint/home-origin.conf`. An isolated `nginx -t` passed using the installed certificates and route include. The server certificate for `mint-home.internal` expires on 2027-11-20. The HOME server CA, public-edge client CA and private client CA are separate. HOME retains the CA signing keys under `/etc/lightning-goats-reserve/origin-pki`.
- **VPS agent:** provide the public-edge client CSR and confirm the VPS WireGuard source address through the agreed exchange. HOME will return the signed client certificate and `home-ca.pem`. Verify the HOME server CA SHA-256 fingerprint through the handoff: `35:B2:7D:DB:63:42:00:99:24:7B:26:E4:FD:A6:67:A0:89:AA:BD:86:0F:5F:A8:39:27:20:87:FD:4E:A6:4C:08`. Enable only the public upstream on the public hostname. For the bounded test, coordinate access with the operator before accepting viewer top-ups.
- The operator completed recovery provisioning. HOME verified the installed encrypted credentials were owned by root with mode 0600. The mainnet reserve synced using the permanent wallet identity. The local issuer is pinned to that identity. Both services are active and enabled at boot. HOME also verified the running reserve's CPU mask after restart.
- An ordinary 8-sat quote produced a signed mainnet BOLT11. Restarting both services preserved the reserve identity, active sat keyset and original invoice. The mint reported the quote unpaid, the reserve reported the invoice pending, and the quote ID matched the public nginx polling route. The issuer rejected a 1,001-sat request. Its per-quote and issued-balance limits are each 1,000 sats for the test. Reserve spending and Lightning melts are disabled.
- The solvency check reported zero sats in the reserve, zero issued liability, zero paid but unissued, and one unpaid 8-sat quote. The full 1,000-sat funding budget remains available. For funded acceptance the operator should create a fresh top-up in Minibits after the canonical endpoint is verified. That gives the phone the claim material for its own proofs.
- HOME DNS still returned `NXDOMAIN` for `mint.lightning-goats.com`. The wallet and issuer serve local Unix sockets. Public TLS, VPS-to-HOME mTLS, a settled Lightning receipt, and Minibits proof ownership remain pending.

### Public origin activation · 2026-11-21

The VPS operator confirmed the public edge uses `10.8.0.1/32` via `wg0`. HOME runs a dedicated [origin service](../../deploy/breez-reserve/lightning-goats-mint-origin.service) with its [single-listener nginx configuration](../../deploy/breez-reserve/public-origin.nginx.conf). Install the unit under `/etc/systemd/system/` and the configuration under `/etc/lightning-goats-reserve/`. It reads certificates and the rendered public route include from systemd credentials. It connects to the local issuer socket as a member of `www-data`. The private receive and funding socket directories are inaccessible to the service.

HOME verified the service was active and enabled, listening only at `10.8.0.6:8792`, with a UFW allow rule restricted to TCP from `10.8.0.1` on `wg0`. The server certificate verified against the published CA fingerprint and the origin rejected a request lacking a client certificate. Port 8793 is closed for the Minibits test. Preserve the private-project CSR on the VPS for a separate activation.

To finish the exchange, provide `public-edge-client.csr` from the VPS handoff directory. HOME signs that request with the public-edge client CA and returns `public-edge-client.pem` and `home-ca.pem`. From the confirmed VPS source, verify `/v1/info` and `/v1/keysets` with the client certificate and confirm rejection of the private routes before enabling the public hostname. HOME's DNS resolver returned `NXDOMAIN` at activation.

### VPS origin acceptance and public TLS check · 2026-11-21

The VPS agent verified the HOME CA fingerprint and the public-edge certificate's match to its private key. From `10.8.0.1` over WireGuard, authenticated requests to `10.8.0.6:8792` returned HTTP 200 for both `/v1/info` and `/v1/keysets`. The responses identified Lightning Goats Cashu Mint on Nutshell 0.22.1, one active sat keyset, a 1,000-sat mint limit and disabled Lightning melts. The public origin returned HTTP 404 for `/wallet/health`.

HOME resolved `mint.lightning-goats.com` to `64.177.40.118`. An HTTPS request failed hostname verification before HTTP. With the mint hostname supplied as TLS SNI, the endpoint served a certificate covering only `blackmetalbuddha.com` and `www.blackmetalbuddha.com`. The VPS agent should verify the intended DNS target and activate the mint virtual host with a matching certificate, then repeat discovery and private-route exclusion checks through the public URL before funding.

### Public wallet acceptance and Lightning melt activation · 2026-11-21

The canonical HTTPS endpoint passed certificate verification, returned mint discovery and the active sat keyset, and excluded private routes. The operator observed a 28-sat Minibits top-up and the corresponding Breez receipt. HOME reconciled 28 sats in the reserve to 28 sats of issued liability, with zero paid-unissued sats and zero pending melts before activation.

At the operator's request HOME enabled reserve spending and Lightning melts. Current limits are 1,000,000 sats per top-up, 10,000,000 sats of aggregate issued balance at quote admission and 1,000 sats per melt. The running reserve reported spending enabled. Public NUT-05 discovery advertised enabled BOLT11 melts; the public melt quote endpoint rejected a malformed invoice, and the private funding endpoint returned 404. The reserve, issuer and public origin services were active and enabled at boot.

A funded outgoing Lightning redemption and recovery of a paired, funded reserve and issuer snapshot remain open. The operator holds the test proofs in Minibits. Reconcile the first funded melt against its original quote, the independent receiving wallet, actual fees, reserve balance and outstanding liability.

### Reserve watchdog and delayed claims

The HOME issuer records the identifiers and amounts of open quotes when a watchdog sample shows a positive reserve surplus. It saves those candidates and the raw balance sample in one SQLite transaction. If the surplus subsequently falls while the reserve stays flat or grows, the watchdog checks which of those specific quotes became issued. It requires the original reserve receipt, matching payment hash and amount, and settlement by the earlier sample. Accepted samples replace the candidate list. A restart retains the last committed sample and its candidates.

A reserve below outstanding liability plus fees, a decline in both reserve and surplus, or a surplus decrease exceeding the verified claims still stops the issuer. An unavailable reserve or missing evidence fails closed. Existing checkpoints without candidate evidence use the original conservative comparison. Keep the watchdog enabled and mismatch overrides disabled.

The package build runs a disposable real-issuer regression for deposit, delayed issuance and restart. It also checks rejection of an incorrect receipt amount, a later receipt, a loss of surplus and an outright deficit. The ordinary issue, swap, restore and melt fixture runs separately.

HOME activated source `c869fe108802170cc6c402450ecb63061ac826c1` for the issuer through the mainnet unit drop-in `watchdog-release.conf`. Its `PYTHONPATH` selects the root-owned versioned issuer tree in `/opt/lightning-goats-reserve/mint-c869fe108802170cc6c402450ecb63061ac826c1/nutshell` and the installed bundle's site-packages. The standard package build includes the same repair. Review this drop-in when replacing the full bundle so it selects the intended issuer revision. The single-service activation retained the original reserve binary, issuer database and credentials. The public discovery and active keyset endpoints passed after activation.

A second timing window occurs during redemption. Nutshell deducts locked proofs before posting final change and input fees, and the reserve may report the Lightning debit on a later sync. The watchdog checks current backing, including the residual value owed on locked inputs even at the maximum quoted fee. It keeps the previous comparison checkpoint while melt proofs are pending or the last reserve sync does not postdate a finalized melt. The integration fixture holds each of these intermediate states across multiple checks, then verifies the reconciled checkpoint and exactly one outgoing payment attempt.

### Private origin activation · 2026-11-21

The operator authorized port 8793. HOME enabled the dedicated `lightning-goats-private-origin.service` with `private-origin.nginx.conf` from `deploy/breez-reserve/`. Nginx listens on `10.8.0.6:8793`, requires a client certificate signed by the private client CA, and permits the VPS source `10.8.0.1`. UFW permits that source on TCP 8793 via `wg0`. HOME verified rejection without a client certificate and observed a ready mainnet receive socket. Verify `GET /wallet/health` from the VPS with the project-receive certificate and its matching private key.

### Public transaction amounts

The HOME operator removed the project-defined principal ceilings for ordinary Lightning top-ups and Cashu-to-Lightning melts. Configure `MINT_MAX_MINT_BOLT11_SAT=0`, `MINT_MAX_MELT_BOLT11_SAT=0` and `MINT_MAX_BALANCE=0`. Public NUT-04 and NUT-05 discovery omit the optional maximum. The reserve accepts positive whole-sat amounts representable in its Lightning invoice format. A melt still requires sufficient proofs to cover principal and fees, an acceptable provider route, and a routing fee within the reserve's 100-sat fee ceiling. The authenticated project-quote interface retains its own request validation.

### Direct operator payouts to Cashu Wallet

In Cashu Wallet on the receiving phone, select the Goats mint and obtain the
wallet’s compressed P2PK receiving public key. The wallet’s Lightning address
and Nostr npub serve different purposes. The 66 hex digits start with 02 or 03.
Use the key for which this wallet can sign Cashu spends. Keep the spending key
and wallet backup on the phone or in its existing private backup.

Set `operator_p2pk` in the root-managed payout configuration (see
`deploy/cyberherd/payout.toml.example`). A configured direct wallet takes
precedence for new feedings. Existing service and core execution controls apply.
Install the updated payout and core binaries and the collector modules together
using the ordinary stopped-writer backup and migration procedure. The core
requires schema 38 and the payout journal requires schema 14. Existing invoices
are bound to Lightning by migration. Keep all journals and keys on rollback.
An older binary may refuse the migrated schema. Use a compatible release to
reconcile outstanding transfers; restoring stale journals can duplicate spends.

The collector reserves the original inputs and private swap outputs before it
contacts the mint to swap. A retry restores the same blinded outputs. P2PK
proofs use the original receiving key even after configuration rotation. Mint
input fees come out of the operator share. Change remains in the original
collection claims. A nonpositive payable amount or fee above the pinned limit
holds the allocation. Check the private payout journal for the feed and status:

```sql
SELECT t.request_id, s.state, s.reason, s.amount_sat, s.fee_sat
FROM payout_operator_ecash_status AS s
JOIN payout_intake_receipts AS t USING (sequence);
```

For a ready transfer, run the installed helper on the VPS as the operator.
Supply the confirmed feed UUID and a new file in a private directory.

```sh
sudo python3 -I -S -B /opt/lightning-goats-project-collector/collector/export_operator_token.py FEED_UUID /root/operator-token.cashu
```

Privately copy the token text into Cashu Wallet to receive it. The phone may be offline when the feeding confirms. Subsequent exports
produce the same token. Ready means issued and available to collect; verify
redemption in Cashu Wallet. Keep tokens out of logs, chat, support tickets and
public overlays. Preserve pending dispositions during an outage and resume
against the original mint. A missing swap response can represent an accepted
swap. Never replace it with a new set of outputs or a Lightning payout.

For a local rehearsal, use the patched Nutshell checkout and its Python
environment from the pinned mint build. Set `PYTHONPATH` to that checkout
and run `bash deploy/breez-reserve/tests/direct_worker_acceptance.sh /absolute/path/to/.venv/bin/python`
as an unprivileged user. The fixture creates disposable credentials, two
local mints, and fresh core, payout and collector databases. To include the
client decoder, set `LG_CASHU_TS_INSTALLATION` to the local npm installation
containing the Minibits-compatible `@cashu/cashu-ts` package. On a phone,
verify the receiving key, paste the privately exported token, and check the
resulting balance at the Goats mint.

#### Automatic delivery to Cashu Wallet

Use `funding_source = "retained_cashu"` in both the core's
`[core_bridge.reward_policy]` and the payout service configuration. An omitted
core selection defaults to Strike and can freeze a zero-value plan even when
the collector owns the feed's ecash. Only an actual CyberHerd reservation
reduces the gross operator allocation. With no payable members the operator
receives all verified proceeds, less the mint's operator swap fee.

To enable automatic delivery, install a reusable, amountless NUT-18 payment
request from Cashu Wallet at
`/etc/lightning-goats-project-collector/operator-payment-request` (root-owned,
group-readable by goats-project-collector). Use the Goats mint and the same
P2PK key as the payout configuration. Install the collector bundle and
`lightning-goats-operator-delivery.service`, with Node installed at
`/usr/bin/node`, and enable the service. After a payout it sends the existing
proofs in a NIP-17 encrypted message to the relays in the request.
A relay acknowledgment is recorded in the private delivery journal.
The wallet must receive and process the message before it can show spendable
funds. A retry sends the original encrypted event. Manual retrieval still
returns the same token and cannot create another payout.

### Converting mint fees to operator ecash

On HOME, the solvency report accepts `--operating-buffer-sat` to estimate the
maximum input fees that could become operator ecash after protecting outstanding
proofs, paid unissued quotes and the specified operating buffer. A stale reserve
sample or pending settlement holds the estimate at zero. The estimate uses
independent reserve and issuer samples and is for planning only.

The HOME issuer reconciles cumulative fees against prior conversions, recorded
issuer expenses, paid unissued quotes and the operating buffer inside its write
transaction. A conversion adds an equal amount of operator proof liability and
leaves the reserve in Breez. The watchdog checks the conversion journal against
the signed sat outputs before subtracting distributed fees from its fee total.
Its reserve checkpoint should show the same backing immediately before and
after issuance when no other transactions intervene. Existing feeder credit
and confirmed-feed allocations are outside this transaction.

The private HOME endpoint and VPS fee worker use a separate shared
`operator-fee-token`. Install it as an encrypted systemd credential on each
host. Add the credential to the HOME mint instance using a drop-in with
`LoadCredentialEncrypted=operator-fee-token:/etc/credstore.encrypted/lightning-goats-reserve/mainnet-operator-fee-token.cred`.
Set `LG_OPERATOR_FEE_RECIPIENT` to the operator wallet's compressed P2PK public
key, `LG_OPERATOR_FEE_BUFFER_SAT` to the approved operating reserve and
`LG_OPERATOR_FEE_MAX_SAT` to the approved per-request ceiling in the HOME mint
environment. Install the private origin configuration with the conversion routes.

On the VPS, set `LG_FEE_BATCH_SAT` in
`/etc/lightning-goats-project-collector/operator-fees.env`. The oneshot unit
uses the existing mint keyset and operator payment request. Run the service
once for the bounded live test. Verify wallet receipt and the HOME backing
checkpoint before enabling `lightning-goats-operator-fees.timer`.
The timer checks every ten minutes. A pending batch keeps its original ID,
blinded outputs, wallet request and delivery envelope across retries. The UTC
date limits creation to one batch per day. Retain the private journal in
`/var/lib/lightning-goats-operator-fees` through upgrade or rollback.
