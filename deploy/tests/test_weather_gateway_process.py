"""Real gateway + Python exporter, synthetic observation/state only.

Set LG_WEATHER_GATEWAY_BIN to the source-built binary to enable these tests.
No installed unit, OpenHAB credential, receiver, or radio hardware is used.
"""
from contextlib import closing
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'deploy/weather'))
import lightning_goats_weather as capture
from serve_snapshot import handler

BINARY = os.environ.get('LG_WEATHER_GATEWAY_BIN')


@unittest.skipUnless(BINARY, 'set LG_WEATHER_GATEWAY_BIN for real gateway integration')
class RealWeatherGatewayTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.db = self.root / 'weather.db'
        self.process = None
        self.exporter = None
        self.thread = None
        self.addCleanup(self.stop_gateway)
        self.addCleanup(self.stop_exporter)
        (self.root / 'openhab-token').write_text('invalid-synthetic-only')
        self.now = int(time.time())
        self.packet = {'model':'Fineoffset-WH65B','id':7,'time':self.stamp(self.now),
                       'temperature_C':20,'humidity':52,'wind_dir_deg':90,'wind_avg_m_s':2,
                       'wind_max_m_s':3,'rain_mm':25.4,'light_lux':12670,'uvi':3}

    def stamp(self, epoch):
        return datetime.fromtimestamp(epoch, timezone.utc).isoformat()

    def start_exporter(self, klass=None):
        self.exporter = HTTPServer(('127.0.0.1', 0), klass or handler(self.db))
        self.thread = threading.Thread(target=self.exporter.serve_forever, daemon=True)
        self.thread.start()

    def stop_exporter(self):
        if self.exporter:
            self.exporter.shutdown()
            self.exporter.server_close()
            self.thread.join(timeout=5)
            self.exporter = None

    def stop_gateway(self):
        if self.process:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)
            self.process = None

    def start_gateway(self):
        with closing(socket.socket()) as sock:
            sock.bind(('127.0.0.1', 0))
            self.port = sock.getsockname()[1]
        config = (ROOT / 'deploy/gateway/config.canary.toml.example').read_text()
        config = config.replace('10.8.0.6:8790', f'127.0.0.1:{self.port}')
        config = config.replace('sqlite:///var/lib/lightning-goats-gateway-canary/gateway.db', 'sqlite://' + str(self.root / 'gateway.db'))
        config = config.replace('http://127.0.0.1:5000/get_received_data', f'http://127.0.0.1:{self.exporter.server_port}/get_received_data')
        # Unreachable fixture-only OpenHAB; weather has no command dependency.
        config = config.replace('http://127.0.0.1:8080/', 'http://127.0.0.1:1/')
        path = self.root / 'config.toml'
        path.write_text(config)
        self.process = subprocess.Popen([str(Path(BINARY).resolve()), '--config', str(path)],
            env=dict(os.environ, CREDENTIALS_DIRECTORY=str(self.root), TOKIO_WORKER_THREADS='2'),
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(100):
            if self.process.poll() is not None:
                self.fail('synthetic gateway exited during startup')
            try:
                if self.get('/healthz')[0] == 200:
                    return
            except OSError:
                pass
            time.sleep(.03)
        self.fail('gateway startup timeout')

    def get(self, route='/v1/weather'):
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        try:
            response = opener.open(f'http://127.0.0.1:{self.port}' + route, timeout=5)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            return response.code, json.load(response)

    def record(self, packet=None, now=None):
        return capture.record_packet(self.db, self.packet if packet is None else packet, 7,
                                     now=self.now if now is None else now)

    def test_actual_normalized_shape_rejected_then_exported_packet_passes_real_adapter(self):
        raw = (ROOT / 'tests/fixtures/weather/deployed-normalized-sanitized.json').read_bytes()
        class Legacy(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200); self.end_headers(); self.wfile.write(raw)
            def log_message(self, *args):
                pass
        self.start_exporter(Legacy)
        self.start_gateway()
        self.assertEqual(self.get()[0], 502)
        self.stop_gateway(); self.stop_exporter()
        self.assertTrue(self.record())
        self.start_exporter(); self.start_gateway()
        status, result = self.get()
        self.assertEqual(status, 200)
        self.assertEqual(result['temperature_f'], 68)
        self.assertEqual(result['wind_speed_mph'], 4.47)
        self.assertEqual(result['wind_direction'], 'E')
        self.assertEqual(result['observed_at'], self.stamp(self.now).replace('+00:00','Z'))
        self.assertEqual(self.get(), (status, result))
        self.stop_gateway(); self.stop_exporter()
        self.start_exporter(); self.start_gateway()
        self.assertEqual(self.get(), (status, result))

    def test_invalid_stale_future_units_and_restored_producer_fail_closed(self):
        self.assertTrue(self.record())
        old = self.root / 'old-producer.db'
        with closing(sqlite3.connect(self.db)) as source, closing(sqlite3.connect(old)) as target:
            source.backup(target)
        self.start_exporter(); self.start_gateway()
        self.assertEqual(self.get()[0], 200)
        for packet in (dict(self.packet, time=None), dict(self.packet, time='bad'),
                       dict(self.packet, time=self.stamp(self.now+60)), dict(self.packet, humidity='bad')):
            self.assertFalse(self.record(packet))
            self.assertEqual(self.get()[0], 502)
        # A record legitimately captured long ago stays stale after producer restart.
        older_db = self.root / 'stale.db'
        self.assertTrue(capture.record_packet(older_db, dict(self.packet,time=self.stamp(self.now-600)),7,now=self.now-600))
        with closing(sqlite3.connect(older_db)) as source, closing(sqlite3.connect(self.db)) as target:
            source.backup(target)
        self.assertEqual(self.get()[0], 502)
        self.assertTrue(self.record())
        with closing(sqlite3.connect(self.db)) as db, db:
            payload = json.loads(db.execute('SELECT payload FROM snapshot').fetchone()[0])
            payload['units']['tempf'] = 'degC'
            db.execute('UPDATE snapshot SET payload=?', (json.dumps(payload),))
        self.assertEqual(self.get()[0], 502)
        newer = dict(self.packet, time=self.stamp(self.now+2))
        self.assertTrue(self.record(newer,now=self.now+2))
        self.assertEqual(self.get()[0], 200)
        # Restoring only the producer cannot roll back the gateway watermark.
        self.stop_gateway(); self.stop_exporter()
        with closing(sqlite3.connect(old)) as source, closing(sqlite3.connect(self.db)) as target:
            source.backup(target)
        self.start_exporter(); self.start_gateway()
        self.assertEqual(self.get()[0], 502)
