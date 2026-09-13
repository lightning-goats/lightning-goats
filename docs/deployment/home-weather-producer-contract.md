# Deployed weather producer compatibility

Implemented preparation artifact; **not installed or activated**. The deployed
receiver, radio service, household clients and gateway configuration are unchanged.
No request was sent to the deployed gateway `/weather` endpoint. Shared Rust files and the public gateway schema
are unchanged. This addresses #21's deployed-source mismatch, not Nostr scheduling.

## Source and actual boundary

Read-only inspection on 2026-09-13 identifies `weather.service` in
`/etc/systemd/system/weather.service`, working directory `/home/sat/bin`, running
Gunicorn with `/home/sat/bin/weather.py`. Source SHA-256:
`1cd65bfe7966da558aed156373501ebfb7116c0dd5f6ca62a31e0901067e1572`.
The radio input is `rtl_weather.service`, same working directory, source
`/home/sat/bin/rtl_weather.py`, SHA-256:
`9f034f9d0ed5912ad5495136bcb6414c20bccb3cb24d1c09622a16c29c9e2777`.
These deployed files are not assumed to match the historical public middleware.
Hashes identify the inspected files; they do not prove every loaded worker has
reloaded that disk generation. No worker was restarted to make that claim.

`receive_url_parameters()` accepts separate WH65B/WH32B/WH31E updates, substitutes
cached values for missing fields, and stores normalized Item-name dictionaries.
`get_all_received_data()` flattens those independent dictionaries. The actual
read returned **32 fields** on this inspection (the earlier checkpoint had 31),
with no `dateutc` or per-field observation time. The sanitized fixture
`tests/fixtures/weather/deployed-normalized-sanitized.json` preserves JSON shape
and field names, replaces sensor identifiers/readings, and remains rejected by
the real gateway with HTTP 502. The field count can change as sensors update;
it is not evidence of freshness.

The deployed radio command uses `rtl_433 -M utc -F json`. `json.loads(line)`
receives the decoder packet, but the outgoing `payload` retains only sensor
values and drops the decoder's `time` metadata. Receiver `_last_update` is
process-local, uses local time without an offset, and precedes field validation.
It cannot certify a coherent restored observation. No timestamp can be recovered
from the existing flattened response alone.

## Implemented project-only observation recorder

`deploy/weather/lightning_goats_weather.py` records an accepted outdoor station
frame at the actual decoder event boundary. It requires the operator's existing
station-ID allowlist and a complete WH65B/WH24 frame. Missing fields are never
filled from cached values. Indoor/foreign-station packets cannot refresh outdoor
weather. Decoder time must be a full UTC date/time; absent, malformed, stale,
future or regressing timestamps disable export rather than substituting now.

Time semantics are explicit: **UTC radio-decoder observation time**, not a
station-measured clock and not HTTP poll, file-read or process-start time. The
export retains `time_basis=radio_decode_utc` and explicit `units`. It uses that
observation as the legacy `dateutc` consumed by the unchanged Rust adapter.
The gateway's existing `observed_at` consequently represents this same decoder
observation for this source; the output does not add a device-measurement claim.
VPS should acknowledge this source interpretation before activation.

| Actual radio field | Export key | Source-proven conversion |
| --- | --- | --- |
| `temperature_C` | `tempf` | C × 1.8 + 32, rounded to 2 decimals |
| `humidity` | `humidity` | percent; deployed rounding/truncation retained |
| `wind_avg_m_s` | `windspeedmph` | m/s × 2.23694, rounded to 2 decimals |
| `wind_max_m_s` | `windgustmph` | same conversion |
| `wind_dir_deg` | `winddir` | rounded degrees, 0–360 |
| `uvi` | `uv` | deployed rounding/truncation to integer index |
| `light_lux` | `solarradiation` | existing estimated lux/126.7, capped 1200 W/m² |

The solar conversion is the deployed estimate, not a new calibration claim.
Rain accumulation, pressure, apparent temperature and indoor values are omitted
because this complete outdoor frame cannot prove their coherent observation
and accumulator semantics. No inferred Fahrenheit is attached to the optional
unitless OpenHAB temperature Item.

SQLite stores one latest export plus a durable observation high-water mark with
WAL/FULL, in one transaction. A malformed selected frame clears availability
without lowering the mark. Only a valid nonregressing real frame restores it.
A duplicate second-level timestamp is allowed, matching the gateway policy;
submillisecond decoder time is normalized to milliseconds once at capture.
The record survives process restarts unchanged. Missing storage is unavailable;
the HTTP reader opens read-only and cannot initialize it or advance timestamps.

`deploy/weather/serve_snapshot.py` provides only the project GET
`/get_received_data` at a configurable **loopback-only** port, proposed 5001.
It returns 503 for missing/invalid/stale state. The existing port 5000 endpoint
and all household field names remain unchanged. The gateway already accepts
this loopback URL/path, so no alias expansion in `src/gateway/weather.rs` is needed.
The observer's bounded SQLite contention wait is 100 ms; filesystem I/O can still
add latency to the radio process, which is part of the required host review.
Errors are generic and caught before continuing the existing radio dispatch.

## Repeatable preparation and verification

```sh
python3 deploy/scripts/prepare-weather-capture-hook.py \
  --source /home/sat/bin/rtl_weather.py --output /protected/new-weather-capture.patch
cargo +1.88.0 build --locked --bin lightning-goats-gateway
LG_WEATHER_GATEWAY_BIN=target/debug/lightning-goats-gateway \
  python3 -B -m unittest discover -s deploy/tests -p 'test_weather*.py' -v
```

The patch helper checks the exact deployed SHA, unique insertion sites, and
Python syntax. It writes an exclusive patch only, never modifies the producer.
The generated observer is optional and catches storage errors; original station
filtering, unit conversion, throttling and household HTTP dispatch remain.
No historical snapshot is imported to make the project export look fresh.

Ten tests pass locally, including **two real gateway process tests**. The current
normalized fixture fails 502; a synthetic coherent recorder snapshot succeeds 200
with correct units. Missing/invalid/future/stale time, partial frames, inconsistent
units and restored older producer state fail closed. Producer/gateway restart
preserves observation time and the gateway's independent high-water mark rejects
a restored older producer snapshot. CI builds the exact candidate's binary and
runs these process tests; ordinary unit discovery skips them unless the binary
is explicitly supplied. No real receiver mutation or radio input injection occurs.

## Host application and rollback gate

Before asking for a host change approval, review the generated patch/library
hashes, actual runtime UID and module search path, ownership/permissions for a
new project state directory, a dedicated read-only exporter identity/unit, and
an exact backup/restore of the original radio script. The original receiver
service and port 5000 listener need no change. Hook installation/restarting the
shared radio service is a separate approved step; no such approval is inferred.

The first approved live observation must establish that accepted decoder packets
actually contain the required UTC `time` field. Until then this is tested source
compatibility, not real input acceptance. If the runtime omits that field, keep
weather unavailable and review a timestamp capture at the decoder event itself;
do not fall back to polling time. Check source identity after station battery/ID
changes and do not silently accept a different station.

Rollback stops only the new exporter, returns only the project gateway weather
URL to its exact prior value, and restores the radio script only if the installed
hook still matches its approved digest and no later changes exist. Preserve
snapshot/evidence files and the existing receiver/household services. Restarting
the radio service to install or remove the hook needs the same reviewed window.
A stale snapshot must remain stale across rollback. #21 stays open pending
producer installation, real-frame/time verification and VPS interpretation ACK.
