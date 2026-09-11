# Weather Overlay Integration

Status: required Phase 1 compatibility behavior.

Tracker: issue #21.

## Existing system

Weather data is currently received and stored by the legacy service in:

```text
lightning-goats/middlware/weather.py
```

The service runs on the in-house OpenHAB/weather host:

```text
10.8.0.6:5000
```

and exposes:

```text
GET /get_received_data
GET /weather?...        # mutating ingestion endpoint
```

The current read URL is:

```text
http://10.8.0.6:5000/get_received_data
```

The current Lightning Goats LNbits extension contains the canonical Phase 1 weather normalization/message/scheduling behavior in:

```text
lightning-goats/lightning_goats_extension/services/weather.py
lightning-goats/lightning_goats_extension/services/messaging.py
lightning-goats/lightning_goats_extension/tasks.py
lightning-goats/lightning_goats_extension/config.py
```

Where README/comments disagree with current code constants/behavior, current code is canonical for the migration baseline.

## Security decision

The public VPS must **not** receive direct TCP access to `10.8.0.6:5000`.

Reason: the same legacy Flask service that exposes read-only `/get_received_data` also exposes the unauthenticated/mutating `/weather` ingestion route. A compromised VPS must not be able to replace weather station state by calling that endpoint.

Instead, the in-house Lightning Goats integration gateway from issue #17 exposes a sanitized read-only API:

```text
GET /v1/weather
```

The gateway may read the legacy weather service locally/trusted-side:

```text
http://127.0.0.1:5000/get_received_data
```

when colocated on `10.8.0.6`, or an equivalent trusted-only local route if the gateway runs adjacent to it.

The gateway must never proxy or expose `/weather` or arbitrary legacy-service paths.

## Normalized weather contract

The gateway should convert the legacy response into a small stable representation similar to:

```json
{
  "temperature_f": 71,
  "humidity_percent": 32,
  "wind_speed_mph": 8,
  "wind_direction": "270",
  "wind_gust_mph": 14.2,
  "uv_index": 5,
  "pressure_relative_inhg": 29.88,
  "rain_hourly_in": 0.0,
  "rain_daily_in": 0.03,
  "solar_radiation_wm2": 612.0,
  "observed_at": "..."
}
```

Exact field names may be adjusted during implementation, but they must remain explicit, typed, bounded, and independent of the legacy Flask schema.

Legacy source fields include:

```text
dateutc
tempf
humidity
winddir
windspeedmph
windgustmph
baromrelin
hourlyrainin
dailyrainin
solarradiation
uv
```

Do not forward the complete unvalidated legacy object directly to the overlay.

## Validation

The gateway should reject or omit invalid fields rather than invent plausible values.

Validate at least:

- finite numeric values;
- sane configured ranges;
- bounded response/body size;
- expected list/dict response shape;
- optional timestamp freshness when `dateutc` can be reliably parsed.

If the weather source is empty, malformed, unavailable, or stale, weather presentation should be skipped/degraded. Weather failure must never affect payment settlement, feed credit, or feeder actuation.

## Overlay message construction

Preserve the current Lightning Goats extension style.

The message begins:

```text
🌤️ Weather Update:
```

Primary fields include:

- temperature;
- humidity;
- wind speed/direction;
- UV index.

Where available, include:

- apparent/feels-like temperature;
- wind gust;
- relative pressure and trend;
- hourly/daily rain;
- solar radiation.

The existing formatter in `lightning_goats_extension/services/weather.py` is the behavioral reference during the port.

## Publication audience

Weather is presentation-only:

```text
weather_status -> overlay only
```

A weather event must never enter the Nostr outbox in Phase 1.

## Periodic informational-message scheduling

The current code defines:

```text
DEFAULT_WEATHER_BROADCAST_INTERVAL = 60 seconds
DEFAULT_WEATHER_BROADCAST_PROBABILITY = 0.4
```

So the migration baseline is:

```toml
[informational]
interval_seconds = 60

[weather]
enabled = true
broadcast_probability = 0.40
```

When both interface-info and weather messaging are enabled, current `tasks.py` behavior is:

1. sleep/evaluate once per configured interval;
2. evaluate interface-info first using the same configured probability;
3. if interface-info is emitted, send no weather message that cycle;
4. if interface-info is not emitted, adjust the conditional weather probability so weather still has the configured 40% **unconditional** chance;
5. emit at most one informational message per cycle.

With the current 0.40 default and both categories enabled, the effective per-cycle outcomes are approximately:

```text
interface info: 40%
weather:        40%
no message:     20%
```

Preserve this user-visible behavior unless the operator explicitly chooses a new policy. Make interval/probability configuration explicit and make tests deterministic by controlling randomness.

## WireGuard/UFW

Use the existing `10.8.0.0/24` WireGuard topology documented in `docs/deployment/wireguard-topology.md`.

From the VPS, the allowed home-side path is the **integration gateway port only**.

Required negative test:

```text
10.8.0.6:5000 -> blocked from VPS
```

Positive weather path:

```text
VPS -> integration gateway -> local 127.0.0.1:5000/get_received_data
```

## Future migration

The legacy Flask receiver may be replaced later. Phase 1 should depend only on the gateway's normalized `/v1/weather` contract, so replacing `middlware/weather.py` does not require changing `lightning-goatsd` or the overlay event contract.

## F12 audit correction: actual age and explicit units

Timestamp validation is mandatory. `dateutc` must parse as RFC3339 with an offset
or `YYYY-MM-DD HH:MM:SS` interpreted as UTC. The gateway checks actual age against
`weather_max_stale_seconds`, rejects observations over 30 seconds in the future,
and persists the last accepted timestamp in its durable SQLite store. Older
observations remain rejected after restart. Equal timestamps remain usable only
while their actual age is within the limit. Invalid values never advance that
watermark. An array is selected by newest parsed timestamp, not array position;
invalid entries fail closed. There is no first-seen freshness grace period.

The implemented normalized contract uses `observed_at` (RFC3339 UTC),
`temperature_f`, `humidity` (percent), `wind_speed_mph`, `wind_direction` (cardinal),
`uv_index`, and optional `apparent_temperature_f`, `wind_gust_mph`,
`pressure_relative_inhg`, `pressure_trend`, `rain_hourly_in`, `rain_daily_in`,
`solar_radiation_w_m2`. Generic old temperature/wind keys are not accepted aliases.
The ambiguous legacy `feelslike` field is omitted; only `feelslikef` declares the
required Fahrenheit unit. OpenHAB's separate display temperature requires explicit
Celsius or Fahrenheit units, converts Celsius, and rejects unitless states.
This does not establish the actual owner's physical result contract.

The daemon validates normalized ranges and actual freshness again, with a
five-minute display ceiling even if the gateway's local tolerance is longer.
Stale replay emits a cursor-preserving skip marker instead of old weather text;
see [overlay stream version 1](overlay-stream.md). The authoritative site's
handling of explicit keys, reset/skip events and visible-weather expiry requires
browser acceptance. No reconstructed website or live source observation is
claimed by the loopback fixture tests.
