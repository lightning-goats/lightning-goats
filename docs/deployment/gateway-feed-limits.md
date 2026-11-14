# Operator-selected feed limits

On 2026-11-08 the operator selected a two-second gateway interval and no hourly
feed cap:

```toml
[feeder]
min_feed_interval_seconds = 2
max_feeds_per_hour = 0
```

Zero explicitly disables only the hourly count limit. Positive caps from 1 to 60
retain their previous behavior. Supported intervals are 2 through 86400 seconds.
Intervals are measured from the most recent durable request/acknowledgement time.
Serialization, safety reads, UUID idempotency, immutable refusals and unresolved
reservations remain effective when the hourly cap is disabled. No database reset
or migration is needed. Existing history and pending UUIDs must be preserved.

The installed older binary rejects these settings; build and install the matching
gateway source before applying them. During replacement stop only the production
gateway, preserve its SQLite state and prior binary/configuration, and resume
using the same database and credential. Validate rate behavior with the harmless
process regressions, never with generated physical requests.

The existing OpenHAB owner has its own start cooldown. Its maintained
`openhab/rules/feeder-owner.js` in Earthship UI must use `COOLDOWN_MS = 2000` for
this policy. Apply only that constant change through supported rule REST while
the gateway is stopped and the owner idle; preserve the original rule, triggers,
conditions, request ledger and one-second OFF backstop.

The VPS daemon also has `feeder.inter_feed_delay_seconds`; set it to 2 separately
to obtain the requested dispatcher pace. The operator owns that VPS change in
this session. Home gateway changes do not alter the VPS configuration or prove
actual feed timing. Physical feeding was already explicitly enabled by the
operator; this update does not introduce a manual test feed.

The subsequent operator-run gateway request and observed physical feed are
recorded in the [deployment evidence](../testing/evidence/home-gateway-feeding-20261108.md).
