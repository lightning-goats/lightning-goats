# Deployment artifacts and isolated verification

Production remains on HOLD. These changes address audit F07 and F13 and prepare
the complete stack for F01 integration review. Other audit findings remain open.

## Source and release gate

The audited `main` was `80ae37c7b20950b345a1510fa16a05708925f8b8`. The full
stack is in `5347733c5fbcde9b57dede3ee5aee33c0c20532b` (merge of #28), including
the audited `8048935c2fea3cf37a0231dd8d5915eaf204209b` candidate and #29/#30
CLN-removal merges. The remediation branch integrates that stack with `main`.
Review the entire integration diff; artifact fixes alone do not justify merging.

The release workflow requires the tagged commit to be an ancestor of `main` and
runs the reusable locked Rust, Security and Deployment artifacts workflows on
that tag. PR checks exercise GitHub's proposed merge result. After merging,
retain checks for the exact resulting `main` commit and tag that commit.

Standalone downloads include all three binaries (`lightning-goatsd`,
`lightning-goatsctl`, `lightning-goats-gateway`), `BUILD-INFO.txt` with full source
commit/target/Rust/Cargo versions, and `SHA256SUMS` covering those files and the
archive. The archive also includes `deploy/`, `docs/`, `AGENTS.md`, `Cargo.lock`
and an internal checksum manifest covering every payload file. It contains
examples, not production secrets or the still-unmigrated website. Install the
daemon/CLI on the VPS; install the gateway only on the trusted host.

From a clean reviewed checkout, build and assemble without installing services:

```sh
cargo build --release --locked --bins
bash deploy/scripts/package-release.sh target/release /tmp/lg-release-artifacts v0.1.0-review
(cd /tmp/lg-release-artifacts && sha256sum --check SHA256SUMS)
python3 deploy/scripts/smoke-release.py /tmp/lg-release-artifacts/lightning-goats-v0.1.0-review-x86_64-linux-gnu.tar.gz "$(git rev-parse HEAD)"
```

Choose a new output directory each run. The smoke test extracts to a temporary
directory, validates completeness/checksums/source identity, and runs only the
three `--help` commands without service configuration or credentials. Checksums
detect corruption; they do not authenticate an untrusted artifact. Execute only
trusted builds. Passing help does not prove service startup, database migration,
credential loading, or communication with live dependencies.

## nginx installation layout

The old examples mixed contexts. nginx defines
[`upstream` in `http`](https://nginx.org/en/docs/http/ngx_http_upstream_module.html#upstream)
and [`location` in `server` or `location`](https://nginx.org/en/docs/http/ngx_http_core_module.html#location).
The corrected layout separates them and quotes regexes with braces.

| Repository example under `deploy/nginx/` | Installation path | Context |
| --- | --- | --- |
| `lightning-goats-http.conf.example` | `/etc/nginx/conf.d/lightning-goats-http.conf` | `http`, once |
| `lightning-goats-production.conf.example` | `/etc/nginx/snippets/lightning-goats-production.conf` | production `server` |
| `lightning-goats-canary.conf.example` | `/etc/nginx/snippets/lightning-goats-canary.conf` | staging `server` |
| `lightning-goats-production-site.conf.example` | `/etc/nginx/sites-available/lightning-goats-production.conf` | `http` |
| `lightning-goats-canary-site.conf.example` | `/etc/nginx/sites-available/lightning-goats-canary.conf` | `http` |

The surrounding `nginx.conf` must include `conf.d/*.conf` and enabled site files
inside `http {}`. The two sites can coexist; shared zones/upstreams appear once.
On systems without `sites-enabled`, include the selected site explicitly inside
`http {}`. Do not also load server snippets through `conf.d`.

Replace staging hostname, TLS paths and static roots with reviewed values.
Provision certificates independently before `nginx -t`; examples contain no
certificate automation. HTTP redirects to the fixed configured HTTPS hostname.
Preserve the host's MIME type include. Review all existing server/location
blocks; conflicting legacy routes must not remain active beside these examples.

Only listed application paths are proxied. Reserved API/OpenHAB/feeder/retired
payment prefixes and missing static paths return 404. GET locations also permit
HEAD using nginx's normal `limit_except GET` behavior. Webhooks require POST,
JSON content type and at most 32 KiB. Health is loopback-only. Rate rejection
returns 429. Public application budgets and durable recovery remain separate.

Run isolated tests before installing any file:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s deploy/tests -v
```

Dependencies: Python 3.10+, nginx with SSL/proxy/rewrite support, and OpenSSL.
Set `NGINX_BIN` if needed. Tests load both shipped sites into one temporary
nginx, replacing only paths, certificates and listener/upstream ports. They bind
loopback, use harmless mocks, and remove only their own process/temp files.
Tests check TLS, redirects, static content, discovery/callback/query forwarding,
method/body/content-type limits, WebSocket upgrade/frame, rate rejection before
upstream contact, health ACLs, and unknown/retired route denial even when matching
static files exist. Archive regressions use harmless executables; CI also builds
and packages all three actual Rust binaries.

Tests use self-signed TLS with trust verification disabled and omit IPv6 listeners
for portability. Public certificate trust/issuance, real DNS/SNI, IPv6 policy,
application abuse controls, browser overlay replay/heartbeat and F08/F11 remain
unverified. Production nginx is never started or reloaded by these scripts.

## Fresh-install acceptance still required

Record clean VM installation of the exact integrated commit with shipped examples.
Install root-owned binaries/config and separate non-admin runtime users. Bind
harmless OpenHAB/Strike first and exercise real daemon/gateway modules. Record
service startup, database migrations, credential permissions and audit matrix.

Backup/restore must retain issued requests, credited settlements, unresolved feed
UUIDs in both stores, and signed outbox bytes. Restore under isolated networking
with actuation disabled. Confirm no duplicate credit, no new UUID/command for an
unresolved actuation, and no re-signing of persisted events. An older database
does not itself authorize replay.

The website, signer/relay, owner contract, credentials and home network remain
acceptance gates. Obtain explicit operator approval separately for tiny real
payment, controlled physical feeding, and DNS/WireGuard cutover.

## Retained staging candidates

The Deployment workflow retains the smoke-tested three-binary package as
`staging-package-<GITHUB_SHA>` for 14 days. Download it from that exact workflow,
verify SHA256SUMS and BUILD-INFO, then run the archive smoke verifier before
staging review. PR builds use GitHub's synthetic merge SHA; record both PR head
and actual artifact source. A passed artifact does not establish service,
credential, network, browser or physical-owner acceptance. The offline paired-store
helper and its limits are documented in `staging-acceptance-evidence.md`.
