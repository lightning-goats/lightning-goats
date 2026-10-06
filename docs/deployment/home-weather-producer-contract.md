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
rollback journaling and synchronous FULL, in one transaction. A malformed selected frame clears availability
without lowering the mark. Only a valid nonregressing real frame restores it.
A duplicate second-level timestamp is allowed, matching the gateway policy;
submillisecond decoder time is normalized to milliseconds once at capture.
The record survives process restarts unchanged. Missing storage is unavailable;
the HTTP reader opens read-only and cannot initialize it or advance timestamps.

`deploy/weather/serve_snapshot.py` provides only the project GET
`/get_received_data` at a configurable **loopback-only** port, proposed 5002.
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

Eleven tests pass locally, including **two real gateway process tests**. The current
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

## Read-only identity correction

Before installation, a real filesystem-permission probe found that the original
WAL database could require creation of `-wal`/`-shm` after the short-lived writer
closed. An exporter with no database/directory write permission failed with
`attempt to write a readonly database`. The recorder now uses SQLite rollback
journaling with synchronous FULL and the same immediate transaction. No `immutable`
flag ignores active writes, and no write permission is granted to the reader.
A subprocess regression runs against mode0444 files in a mode0555 directory
(dropping root when necessary), reads the real capture module successfully and
asserts no sidecar creation. It fails before the mode correction and passes after.
This corrects preparation source only; no deployed weather database or service
has been changed. Existing real gateway stale/restore tests remain required.

## Concrete installation layout for review

The inspected radio and receiver services both run as `sat:sat` from
`/home/sat/bin`. Keep that household service identity unchanged. The generated
hook now loads the project module from an explicit root-managed absolute path,
without adding a user-site module, changing Python search paths or editing the
receiver. Before applying, require the pinned radio-source digest still matches
and retain an exact private backup with ownership/mode/hash.

Proposed project layout:

| Path or identity | Required state |
| --- | --- |
| `lightning-goats-weather` | New locked system user/group; no sudo or supplementary groups |
| `/usr/local/lib/lightning-goats-weather/` | New root:root 0755 directory; two reviewed Python modules root:root 0644 |
| `/var/lib/lightning-goats-weather/` | New sat:lightning-goats-weather 2750 directory; reader cannot create/remove files |
| `snapshot.db` | New empty sat:lightning-goats-weather 0640 file, initialized only by the radio observer |
| `lightning-goats-weather.service` | Reviewed root-owned system unit; fixed loopback 5002, no credentials or write paths |

`deploy/systemd/lightning-goats-weather.service` provides the concrete exporter
unit. It uses a separate non-admin reader, read-only system protection, no
capabilities and loopback-only networking. The writer retains only project-state
write access through its existing identity; no existing user's groups change.
Missing/invalid data returns 503, never an invented fresh observation.

Apply remains a separate operator-approved window: verify absence of all new
paths/identity and port 5002, stage modules/unit and empty restricted state, verify
the unit, back up the exact radio source, apply the generated digest-pinned patch,
restart only the radio service, then start only the project exporter. Validate a
real complete decoder frame/time and readonly reader permissions before changing
only the gateway's weather URL. Do not enable boot startup or claim weather
acceptance before that validation. Preserve existing receiver port 5000/clients.

Rollback stops the new exporter and restores only the exact approved radio patch
if its digest still matches, then restarts that radio service in the same approved
window. Restore the prior project gateway URL if changed. Preserve project data,
backup and evidence. No household user/group, receiver or network-policy rollback
is required because none is part of this plan. No installation was performed.

## Fresh port-conflict correction — 2026-09-13

Read-only installation preflight found the originally proposed127.0.0.1:5001
already held by an existing Docker listener. It remains untouched. The exporter
default and reviewed unit now select127.0.0.1:5002, observed unbound during this
preflight; verify it remains free immediately before approved startup. Existing
receiver5000 is unchanged. This corrects preparation, not installed behavior.

The source still matches9f034f9d0ed5912ad5495136bcb6414c20bccb3cb24d1c09622a16c29c9e2777.
Protected original/proposed source and patch are prepared; proposed radio script
SHA256deaf48dc9e35dabb06c93bbd484c8750479f25cc9b3e50f7ea0acebb2df7ade6,
patch SHA25653dcb69e90c9dc0da4874dc7dc57510df100b2020cfbfe26c064e68fc9d2cf6c.
Both existing services are active as sat:sat; new project paths/identity are
absent. The patch imports the root-managed observer and records only coherent
decoder events; no poll-time freshness or receiver change. Backup hashes and
original ownership/mode are retained privately. Installation, radio restart and
real-frame acceptance remain pending; recheck all drift before applying.
