#!/usr/bin/env python3
"""Optional loopback GET-only project export. No weather/OH mutation endpoint."""
import argparse
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
from pathlib import Path
from lightning_goats_weather import read_snapshot


def handler(database):
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            if self.path != '/get_received_data':
                self.send_error(404)
                return
            try:
                body = json.dumps(read_snapshot(database), allow_nan=False).encode()
                status = 200
            except Exception:
                body = b'{"status":"unavailable"}'
                status = 503
            self.send_response(status)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(body)))
            self.send_header('Cache-Control', 'no-store')
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *args):
            pass
    return Handler


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--database', required=True, type=Path)
    parser.add_argument('--port', type=int, default=5001)
    args = parser.parse_args()
    HTTPServer(('127.0.0.1', args.port), handler(args.database)).serve_forever()
