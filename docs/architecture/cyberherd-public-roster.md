# Read-only CyberHerd roster and OBS state

The isolated intake worker has an optional `[view]` section. It is absent by
default and the example remains disabled. An enabled listener must use an
explicit loopback address and nonzero port. This adds no public mutation route,
signing credential, provider adapter or feeder capability. The inactive nginx
snippet in `deploy/cyberherd/readonly-nginx.conf.example` proxies only the exact
read endpoint after separate installation review.

## Projection and HTTP contract

`GET /api/v1/cyberherd` returns version 1, generation time, active roster, current
authenticated targets and actual core availability. The background producer
refreshes every five seconds. Requests use only its immutable cache; they never
query or mutate membership, contact relays/providers, or dispatch food. Reports
older than fifteen seconds return 503. Unknown discovery is `targets: null`,
distinct from a successful discovery with no current targets (`[]`). Discovery
observations expire after sixty seconds and cannot move backward in time.

The roster carries owner/cycle/revision, configured capacity/minimum/member pool,
active count and ranked members. Members expose public key, human display name,
sanitized public HTTPS picture, exact contribution/points/share and social awards.
It omits payment sources, payout IDs, Lightning addresses, credentials and
private commands. `share_eligible` describes allocation eligibility, never
provider execution or promised receipt of a treat. Ranking uses the same exact
point weights as accounting with public-key ties; shares use the existing
six-decimal allocator. Amounts and fractional values use canonical decimal
strings so browser floating point cannot round large sat amounts.

Pages contain at most fifty members. `offset` is bounded to 1,000 and `limit`
to 1..50. The optional `revision` pins later pages; changed state returns 409.
Unknown query fields fail. Responses are no-store and bounded to 96 KiB;
query length, concurrent reads and handler duration are bounded. There is no
per-request full-roster serialization, admin route or browser-supplied owner.

## OBS capability and reconnects

The optional `overlay` flag sends at most the first ten ranked members through
a typed intake-only private bridge method. The core checks its pinned owner,
current epoch, bounded packet, canonical scores/order and monotone cycle/revision.
Payout peers cannot call it. The packet cannot contain an event name, food proof,
money mutation, reset command or public-message request.

A successful import atomically appends one `cyberherd_roster` display event and
stores immutable exact acknowledgment plus current state. A lost ACK returns the
original event sequence. An old exact retry cannot replace a newer snapshot;
changed old revisions are rejected. Current roster is read in the same SQLite
snapshot as the overlay sequence, feed credit and unresolved feeding UUID.
Reconnect therefore restores cards without replaying payment or physical effects.
An empty roster clears cards, including after a cycle reset. Core financial
accounting and Nostr outboxes remain unchanged by a roster import.

## Static page and activation

`web/cyberherd.html` displays ranked contributions, exact points/shares, capacity,
joining rules and current Nostr targets. Names use text nodes; amounts use
BigInt. Current target links are encoded with the existing local Nostr bundle.
Browser requests are same-origin, read-only, size/time bounded and reject
redirects. Failed/stale data hides the roster instead of showing it as current.
`cyberherdEnabled` defaults to false and the home-page link stays hidden; with
that flag false the page makes no API request. No private key or login field is
present. Authenticated member/operator interfaces are separate remaining work.

The source tests use temporary databases, actual private IPC with synthetic
identities, a loopback HTTP server and fully mocked browser traffic. They are
not installation, public relay, provider-payment or physical-feeding evidence.
New roles, API proxy and feature flags require separate installed qualification;
this source does not enable them on the live service.
