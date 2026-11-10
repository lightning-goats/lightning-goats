# Public read-only CyberHerd roster

This stage exposes the already running membership projection and its existing
static page. It repairs `/leaderboard` links in the retained message templates.
It adds no signer, operator authorization, financial budget or payout capability.
The core and membership service configurations and units stay unchanged.

The exact optional snippet `deploy/cyberherd/readonly-nginx.conf.example` proxies
only `GET /api/v1/cyberherd` to the loopback view. Pagination/revision queries are
preserved and all responses are no-store. Existing application query/concurrency/
time/size/freshness limits and nginx/WAF rules still apply. Mutation methods are
denied at nginx, and operator/prefix routes retain the existing 404 boundary.
Exact `/leaderboard` and `/cyberherd` aliases redirect to `/cyberherd.html` ahead
of the retired-path regex; they expose no generic API prefix.

Prepare a new static release by copying the actual current public tree. Replace
only source-matched `index.html`/`site.js`, add the three roster page/scripts and
merge `cyberherdEnabled: true`, `cyberherdAuthEnabled: false` into the existing
site configuration. Preserve the current payment/feeding flags and every OBS,
artwork, vendor and NIP-05 file. The home-page change adds only the roster link.
Do not deploy the source's default-off payment configuration over the live one.
Do not install the operator page/snippet or enable its auth flag in this stage.

Before privileged installation, bind source files to independently fetched
GitHub blobs and the previously qualified immutable source. Hash the complete
old/new public trees, exact old/new nginx site and new snippet in a reviewable
manifest. Install as data under a new protected release root, retain the old
root and original site, verify root ownership/permissions and restore labels.
Use an authenticated recoverable file transaction for the single existing site
file; the new snippet must be unused and exactly match its reviewed bytes.
`nginx -t` must pass before reload. A refused configuration/reload restores the
old site/root and removes only the exact newly installed snippet. Application
binaries, configurations, journals, PIDs and public payment routes are untouched.

`deploy/tests/test_cyberherd_readonly_nginx.py` runs actual isolated nginx against
a synthetic backend. It covers exact reads and preserved queries, both aliases
against the retired regex, all mutation methods, operator/prefix denial and
unavailable-backend propagation. The existing mocked-browser roster suite covers
feature gates, safe/exact rendering, no payment operations and empty-state
clearing. Actual acceptance must additionally check the public HTTPS aliases,
current roster/cards/targets, disabled auth/operator paths, unchanged LNURL/OBS
and application PIDs, and WAF denial of an actual malicious read query. Never
relax the WAF merely to make a new route pass.
