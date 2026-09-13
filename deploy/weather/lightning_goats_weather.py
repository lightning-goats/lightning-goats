"""Project-only durable radio observation export; no OpenHAB writes or credentials.

Input is a real rtl_433 packet after the existing station allowlist. `time` is
UTC radio-decoder observation time (-M utc), NOT station measurement or poll time.
The legacy receiver and its household read shape are left unchanged.
"""
from contextlib import closing
from datetime import datetime, timezone
import json
import math
import re
from pathlib import Path
import sqlite3
import time

MAX_STALE_SECONDS = 300
UNITS = {'tempf': 'degF', 'humidity': 'percent', 'windspeedmph': 'mph',
         'windgustmph': 'mph', 'winddir': 'degree', 'uv': 'index',
         'solarradiation': 'W/m2'}
REQUIRED = ('temperature_C', 'humidity', 'wind_dir_deg', 'wind_avg_m_s',
            'wind_max_m_s', 'rain_mm', 'light_lux', 'uvi')


def number(value, low, high):
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError('radio field must be numeric')
    if not math.isfinite(value) or not low <= value <= high:
        raise ValueError('radio field outside documented range')
    return float(value)


def epoch_from_radio(raw):
    if not isinstance(raw, str) or not 1 <= len(raw) <= 64:
        raise ValueError('missing radio observation time')
    if not re.fullmatch(r'\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}:\d{2}(?:\.\d{1,6})?(?:Z|\+00:00)?', raw):
        raise ValueError('radio timestamp requires full UTC date and time')
    observed = datetime.fromisoformat(raw.replace('Z', '+00:00'))
    # This is source-pinned to rtl_433 -M utc, not local-time guessing.
    if observed.tzinfo is None:
        observed = observed.replace(tzinfo=timezone.utc)
    if observed.utcoffset().total_seconds() != 0:
        raise ValueError('radio timestamp must be UTC')
    return observed.timestamp()


def normalize_packet(packet, now):
    epoch = epoch_from_radio(packet.get('time'))
    epoch = math.floor(epoch * 1000) / 1000
    if epoch < 0 or epoch > now + 30 or now - epoch > MAX_STALE_SECONDS:
        raise ValueError('radio observation stale or future')
    if any(key not in packet for key in REQUIRED):
        raise ValueError('incomplete radio observation; no cached field substitution')
    temp = number(packet['temperature_C'], -73, 65) * 1.8 + 32
    if not -100 <= temp <= 150:
        raise ValueError('temperature outside gateway range')
    humidity = number(packet['humidity'], 0, 100)
    direction = number(packet['wind_dir_deg'], 0, 360)
    uv = number(packet['uvi'], 0, 50)
    # Existing receiver intentionally truncates fractional humidity/UV values.
    # Keep that interpretation explicit; wind direction follows RTL rounding.
    result = {'dateutc': datetime.fromtimestamp(epoch, timezone.utc).isoformat(timespec='milliseconds').replace('+00:00', 'Z'),
              'tempf': round(temp, 2), 'humidity': int(round(humidity, 2)),
              'windspeedmph': round(number(packet['wind_avg_m_s'], 0, 111.76) * 2.23694, 2),
              'windgustmph': round(number(packet['wind_max_m_s'], 0, 134.11) * 2.23694, 2),
              'winddir': round(direction), 'uv': int(round(uv, 2)),
              'solarradiation': round(min(number(packet['light_lux'], 0, 1e7) / 126.7, 1200), 2),
              'time_basis': 'radio_decode_utc', 'units': UNITS}
    number(packet['rain_mm'], 0, 1e7)  # Required station frame; no rain accumulator is invented.
    return epoch, result


def record_packet(path, packet, station_id, *, now=None):
    """Called once per accepted decoded packet, never from an HTTP read/startup.

Foreign models/IDs leave the selected sensor untouched. A malformed selected
frame invalidates export without advancing the durable high-water mark. A later
valid, nonregressing selected observation can restore availability.
"""
    if not isinstance(packet, dict) or type(station_id) is not int or type(packet.get('id')) is not int:
        return False
    if packet.get('model') not in ('Fineoffset-WH65B', 'Fineoffset-WH24') or packet.get('id') != station_id:
        return False
    now = time.time() if now is None else now
    try:
        epoch, snapshot = normalize_packet(packet, now)
    except (ValueError, TypeError, OverflowError):
        epoch, snapshot = None, None
    with closing(sqlite3.connect(path, timeout=0.1)) as db, db:
        db.execute('PRAGMA journal_mode=WAL')
        db.execute('PRAGMA synchronous=FULL')
        db.execute('CREATE TABLE IF NOT EXISTS snapshot (singleton INTEGER PRIMARY KEY CHECK(singleton=1), high_water REAL NOT NULL, payload TEXT)')
        db.execute('BEGIN IMMEDIATE')
        previous = db.execute('SELECT high_water FROM snapshot WHERE singleton=1').fetchone()
        floor = previous[0] if previous else -1
        if epoch is None or epoch < floor:
            db.execute('INSERT INTO snapshot VALUES (1, ?, NULL) ON CONFLICT(singleton) DO UPDATE SET payload=NULL', (floor,))
            return False
        db.execute('INSERT INTO snapshot VALUES (1, ?, ?) ON CONFLICT(singleton) DO UPDATE SET high_water=excluded.high_water, payload=excluded.payload',
                   (epoch, json.dumps(snapshot, allow_nan=False, separators=(',', ':'))))
    return True


def read_snapshot(path, *, now=None):
    """Read-only; restart and polling never manufacture an observation timestamp."""
    now = time.time() if now is None else now
    with closing(sqlite3.connect(Path(path).absolute().as_uri() + '?mode=ro', uri=True, timeout=2)) as db:
        row = db.execute('SELECT high_water, payload FROM snapshot WHERE singleton=1').fetchone()
    if not row or row[1] is None:
        raise ValueError('weather observation unavailable')
    epoch, raw = row
    if epoch > now + 30 or now - epoch > MAX_STALE_SECONDS:
        raise ValueError('weather observation stale or future')
    if len(raw.encode()) > 16384:
        raise ValueError('weather snapshot too large')
    result = json.loads(raw)
    if result.get('time_basis') != 'radio_decode_utc' or result.get('units') != UNITS:
        raise ValueError('weather schema/units mismatch')
    if epoch_from_radio(result.get('dateutc')) != epoch:
        raise ValueError('weather timestamp mismatch')
    return result
