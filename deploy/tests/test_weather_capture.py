import copy
from datetime import datetime, timezone
import importlib.util
import json
from pathlib import Path
import sqlite3
import sys
import tempfile
import unittest

WEATHER = Path(__file__).resolve().parents[1] / 'weather'
sys.path.insert(0, str(WEATHER))
import lightning_goats_weather as capture

NOW = 1800000000
PACKET = {'model': 'Fineoffset-WH65B', 'id': 7,
          'time': datetime.fromtimestamp(NOW, timezone.utc).isoformat(),
          'temperature_C': 20, 'humidity': 52.34, 'wind_dir_deg': 90,
          'wind_avg_m_s': 2, 'wind_max_m_s': 3, 'rain_mm': 25.4,
          'light_lux': 12670, 'uvi': 3.2}


class WeatherCaptureTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name) / 'weather.db'

    def record(self, packet=None, now=NOW):
        return capture.record_packet(self.path, PACKET if packet is None else packet, 7, now=now)

    def test_coherent_packet_units_and_read_restart_do_not_refresh_time(self):
        self.assertTrue(self.record())
        first = capture.read_snapshot(self.path, now=NOW)
        self.assertEqual(first['tempf'], 68)
        self.assertEqual(first['humidity'], 52)
        self.assertEqual(first['windspeedmph'], 4.47)
        self.assertEqual(first['windgustmph'], 6.71)
        self.assertEqual(first['solarradiation'], 100)
        self.assertEqual(first['time_basis'], 'radio_decode_utc')
        # Each reader opens a new process-independent DB connection; no cached clock.
        self.assertEqual(capture.read_snapshot(self.path, now=NOW + 299), first)
        with self.assertRaises(ValueError):
            capture.read_snapshot(self.path, now=NOW + 301)

    def test_missing_invalid_future_stale_time_never_uses_poll_time(self):
        for when in (None, 'invalid', '2027-01-15T08:00:00+02:00',
                     datetime.fromtimestamp(NOW+31, timezone.utc).isoformat(),
                     datetime.fromtimestamp(NOW-301, timezone.utc).isoformat()):
            with self.subTest(time=when):
                self.assertTrue(self.record())
                self.assertFalse(self.record(dict(PACKET, time=when)))
                with self.assertRaises(ValueError):
                    capture.read_snapshot(self.path, now=NOW)

    def test_restart_regression_invalidates_without_lowering_high_water(self):
        self.assertTrue(self.record())
        older = dict(PACKET, time=datetime.fromtimestamp(NOW-1, timezone.utc).isoformat())
        self.assertFalse(self.record(older))
        with sqlite3.connect(self.path) as db:
            self.assertEqual(db.execute('SELECT high_water FROM snapshot').fetchone()[0], NOW)
        with self.assertRaises(ValueError):
            capture.read_snapshot(self.path, now=NOW)
        self.assertTrue(self.record(dict(PACKET, time=datetime.fromtimestamp(NOW+1, timezone.utc).isoformat()), now=NOW+1))

    def test_partial_or_bad_units_input_does_not_mix_cached_values(self):
        for key, value in [('temperature_C', '68 F'), ('humidity', float('nan')),
                           ('wind_avg_m_s', -1), ('uvi', True), ('light_lux', float('inf'))]:
            self.assertTrue(self.record())
            self.assertFalse(self.record(dict(PACKET, **{key: value})))
            with self.assertRaises(ValueError):
                capture.read_snapshot(self.path, now=NOW)
        partial = copy.deepcopy(PACKET)
        del partial['humidity']
        self.assertFalse(self.record(partial))

    def test_foreign_station_and_indoor_update_cannot_refresh_outdoor(self):
        self.assertTrue(self.record())
        before = capture.read_snapshot(self.path, now=NOW)
        self.assertFalse(self.record(dict(PACKET, id=8), now=NOW+300))
        self.assertFalse(self.record(dict(PACKET, model='Fineoffset-WH32B'), now=NOW+300))
        self.assertEqual(capture.read_snapshot(self.path, now=NOW+300), before)

    def test_missing_store_is_unavailable_and_reader_never_creates_it(self):
        with self.assertRaises(sqlite3.OperationalError):
            capture.read_snapshot(self.path, now=NOW)
        self.assertFalse(self.path.exists())

    def test_inconsistent_persisted_units_and_time_rejected(self):
        self.assertTrue(self.record())
        for key, value in [('units', {'tempf': 'degC'}), ('dateutc', '2020-01-01T00:00:00Z')]:
            self.assertTrue(self.record())
            with sqlite3.connect(self.path) as db:
                data = json.loads(db.execute('SELECT payload FROM snapshot').fetchone()[0])
                data[key] = value
                db.execute('UPDATE snapshot SET payload=?', (json.dumps(data),))
            with self.assertRaises(ValueError):
                capture.read_snapshot(self.path, now=NOW)

    def test_submillisecond_decoder_time_is_stably_serialized(self):
        packet = dict(PACKET, time=datetime.fromtimestamp(NOW, timezone.utc).isoformat().replace('+00:00', '.123456+00:00'))
        self.assertTrue(self.record(packet, now=NOW+1))
        result = capture.read_snapshot(self.path, now=NOW+1)
        self.assertTrue(result['dateutc'].endswith('.123Z'))
