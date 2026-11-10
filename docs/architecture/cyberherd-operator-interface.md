# CyberHerd Nostr sign-in and operator commands

The optional `[view.auth]` section adds NIP-98 requests to the isolated loopback
view listener. It pins one canonical public HTTPS origin and an explicit list of
operator public keys. An empty allowlist permits only signed self reads. This
configuration is absent by default; `cyberherdAuthEnabled` also defaults false.
The optional nginx snippet is an inactive example, not an installed route.

## Request authority

The browser reuses the site's injected NIP-07 extension or existing Nostr Connect
login (`connect,extension`, only `sign_event:27235`). It never requests a raw nsec,
project key, wallet key or household credential. NIP-98 events go only in the HTTP
Authorization header, never to relays. The browser verifies the returned native
signature and exact kind/time/content/tags/selected public key before sending.

The server verifies canonical ID/BIP-340 signature, kind 27235, empty content,
60-second clock window, singleton absolute URL including query, exact method,
and mandatory SHA-256 body hash for commands. It constructs the URL from the
configured origin and original request URI, never Host or forwarded headers.
Duplicate tags, extra fields in commands, wrong origin, changed body or method,
unsupported methods and oversized headers/body are rejected. GET bodies are
refused, including chunked bodies. Browser Origin, when present, must match.

A bounded durable nonce table prevents the same authorization event from being
accepted twice, including across restart. Only expired ephemeral nonce records
can be pruned. Mutation nonce consumption commits with the actual command,
audit, membership and presentation event. A failed transaction consumes neither
nonce nor command. Repeated commands use a fresh NIP-98 event and the same UUID,
actor, exact body and expected revision; a committed retry returns its original
receipt before current-state checks. Changed intent conflicts. Commands and their
audit records are retained and immutable; expiring an HTTP nonce never makes a
membership command new again.

Four concurrent authenticated requests, short body/handler deadlines, strict
query limits and 16-KiB command/96-KiB response limits bound the listener. The
nginx example adds exact-path method and request-rate constraints. There are no
cookies, ambient bearer sessions, wildcard admin proxy or caller-selected owner.
Authorization headers and command bodies must not enter edge access logs.

## Routes and mutable authority

- `GET /api/v1/cyberherd/me` returns only the signer's own membership status,
  exact score/lifetime sats, social awards, current cycle/revision and operator
  boolean. It does not expose other members or private source/payment records.
- `GET /api/v1/cyberherd/operator/state` requires a pinned operator and returns
  settings, revision, paginated active/inactive/banned member profiles and template
  category states. Pages contain at most twenty members, with optional revision
  pinning. Sats remain decimal strings.
- `GET /api/v1/cyberherd/operator/templates` returns at most five retained variants
  of an exact category, including disabled/deleted state and revision. Optional
  revision pinning prevents editing a silently changed category.
- `POST /api/v1/cyberherd/operator/command` accepts a UUID, expected revision and
  one strict action: membership settings, activate/update profile, assign score
  total, remove, ban, unban, new cycle or safe template edit. It cannot change
  owner/operator identities, core binding, receive/spend credentials, physical
  limits, execution budgets or signing policy.

Commands update membership/settings, exact share calculation, audit receipt and
optional frozen member notice in one transaction. Desired score totals are
assigned, not incremented; manual scores never become paid credit, lifetime
payment history or provider funding. Capacity reduction cannot evict members
implicitly. Bans require a separate unban before activation. Activation promotes
retained current-cycle social types once; repeated activation cannot award them
again. Removals/bans clear current score while retaining cycle awards/lifetime
history. Settings preserve zero allocations. Template bounds, safe placeholders,
category opt-out and deletion tombstones remain authoritative.

Activation and score changes require fresh confirmed unpaused core status.
Removals, bans, unbans, settings, templates and manual reset do not require the
feeder to be unpaused. A manual reset starts a fresh cycle and clears current
scores/awards/evidence while preserving paid state, receipt history, bans and
lifetime totals. It creates no public reset note. Incoming events older than
its new cutoff cannot earn a new award after reset. Already queued pre-command
social/payment work and deferred interaction receipts become durably discarded
in that same transaction, including same-second sources. Their original input,
payment and source identities remain retained; recovery cannot re-admit them.

## Ordering and messages

Before a new roster/settings/reset command, the service obtains an authenticated
core high-water observation matching the pinned epoch/UID and intake cursor. The
same mutation transaction rechecks that cursor and a five-second freshness bound.
It refuses to pass any pending confirmed-feeding barrier. Thus an HTTP edit cannot
silently enter a feeding snapshot that is already waiting in ordered intake.
Template edits do not change membership/funding and need no core ordering read.
Known committed retries recover their original receipt even if intake is now
behind or the core is unavailable. Missing ordering/safety data holds new edits;
it never fabricates payment or food evidence.

Manual activations/removals use the editable legacy `member_added`/`member_removed`
categories. Explicit score edits use a separate editable `member_score_updated`
category without a received-payment claim. Changed allocations append the same
frozen compact roster footer; unchanged allocations do not. Footer names add no
extra Nostr mentions. Empty herds are explicit. Activation establishes a genuine
new conversation from authenticated current targets when available; removals and
score updates preserve the real existing branch and wait for joining receipts.
Messages use the existing exact note/chat/overlay outboxes and pause gates.
They do not create a core Nostr job, debit, funding reservation or feeder request.

The browser retains an unacknowledged command body/UUID in per-tab session storage
and offers an explicit retry using fresh authorization. It does not retry mutations
automatically. The operator checks current state before discarding a pending retry.
Server revision checks reject stale pages/actions. Disconnect/account changes
remove active authorization; controls require the server's operator decision.

## Verification and activation boundary

Native signature, temporary-store, loopback/private-IPC and mocked-browser tests
cover the contract. They do not qualify an installed household signer, public
origin/WAF, new UIDs, provider spending or physical feeding. The API proxy and
feature flags stay uninstalled/default-off until separate qualification. The full
CyberHerd port also requires restore/retention, dependency remediation and actual
installed identity/network/provider/NIP-46 checks and explicit activation.

Protocol references: [NIP-98](https://github.com/nostr-protocol/nips/blob/master/98.md)
and [NIP-07](https://github.com/nostr-protocol/nips/blob/master/07.md).
