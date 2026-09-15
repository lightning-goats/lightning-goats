#!/usr/bin/env python3
"""Prepare/run a loopback staging response-loss proxy. No automatic retries.

Default is a plan only. --serve requires the separately approved harmless path.
This proxy cannot authenticate the owner or prove command delivery; retain HOME
captures. Each restart needs a new exclusive journal, preserving prior evidence.
"""
import argparse
import base64
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import ipaddress
import json
import os
from pathlib import Path
import threading
import time
from urllib.parse import urlsplit
import uuid

LIMIT = 16384
NETWORKS = tuple(ipaddress.ip_network(n) for n in
                 ('127.0.0.0/8', '10.0.0.0/8', '172.16.0.0/12', '192.168.0.0/16'))


def upstream_origin(raw):
    parsed = urlsplit(raw)
    address = ipaddress.IPv4Address(parsed.hostname or '')
    if (parsed.scheme != 'http' or parsed.username is not None
            or parsed.password is not None or parsed.path not in ('', '/')
            or parsed.query or parsed.fragment or parsed.port is None
            or not 1 <= parsed.port <= 65535
            or not any(address in network for network in NETWORKS)):
        raise ValueError('upstream requires a private IPv4 HTTP origin and explicit port')
    return str(address), parsed.port


def allowed_path(method, path):
    if method == 'GET' and path in ('/healthz', '/v1/feeder/override',
                                    '/v1/temperature', '/v1/weather'):
        return True
    prefix = '/v1/feeder/request/'
    if method not in ('GET', 'POST') or not path.startswith(prefix):
        return False
    raw = path[len(prefix):]
    try:
        return str(uuid.UUID(raw)) == raw
    except ValueError:
        return False


class Journal:
    def __init__(self, path, run_id, upstream, drop_posts):
        path = Path(path)
        if not path.is_absolute() or path.parent.resolve() != path.parent:
            raise ValueError('journal needs an absolute canonical existing parent')
        self.lock = threading.Lock()
        self.failed = False
        self.fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        try:
            self.record(stage='start', run_id=str(uuid.UUID(run_id)),
                        upstream=list(upstream), drop_post_responses=drop_posts)
            parent = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
            try:
                os.fsync(parent)
            finally:
                os.close(parent)
        except BaseException:
            os.close(self.fd)
            raise

    def record(self, **event):
        with self.lock:
            if self.failed:
                raise OSError('journal previously failed; restart requires review')
            try:
                event['at_ns'] = time.time_ns()
                data = (json.dumps(event, allow_nan=False, separators=(',', ':')) + '\n').encode()
                view = memoryview(data)
                while view:
                    count = os.write(self.fd, view)
                    if count <= 0:
                        raise OSError('short journal write')
                    view = view[count:]
                os.fsync(self.fd)
            except BaseException:
                self.failed = True
                raise

    def close(self):
        os.close(self.fd)


class Proxy(ThreadingHTTPServer):
    daemon_threads = False

    def __init__(self, port, upstream, journal, drop_posts=False):
        self.upstream = upstream
        self.journal = journal
        self.drop_posts = drop_posts
        self.slots = threading.BoundedSemaphore(16)
        super().__init__(('127.0.0.1', port), Handler)

    def process_request(self, request, client_address):
        if not self.slots.acquire(blocking=False):
            self.shutdown_request(request)
            return
        try:
            super().process_request(request, client_address)
        except BaseException:
            self.slots.release()
            raise

    def process_request_thread(self, request, client_address):
        try:
            super().process_request_thread(request, client_address)
        finally:
            self.slots.release()


class Handler(BaseHTTPRequestHandler):
    def setup(self):
        super().setup()
        self.connection.settimeout(10)

    def do_GET(self):
        self.forward()

    def do_POST(self):
        self.forward()

    def forward(self):
        self.close_connection = True
        lengths = self.headers.get_all('Content-Length', [])
        if (not allowed_path(self.command, self.path)
                or self.headers.get_all('Transfer-Encoding')
                or lengths not in ([], ['0']) or self.headers.get_all('Expect')):
            self.send_error(400)
            return
        exchange = str(uuid.uuid4())
        connection = None
        try:
            # Persist intent BEFORE any upstream socket is opened. Intent alone
            # cannot prove whether a request was delivered after a crash.
            self.server.journal.record(stage='intent', exchange=exchange,
                                       method=self.command, path=self.path)
            connection = http.client.HTTPConnection(*self.server.upstream, timeout=145)
            connection.request(self.command, self.path, body=None,
                               headers={'Content-Length': '0', 'Connection': 'close'})
            response = connection.getresponse()
            body = response.read(LIMIT + 1)
            if len(body) > LIMIT or 300 <= response.status < 400:
                raise ValueError('oversized or redirected upstream response')
            drop = self.server.drop_posts and self.command == 'POST'
            self.server.journal.record(stage='response', exchange=exchange,
                                       status=response.status,
                                       body_base64=base64.b64encode(body).decode(),
                                       disposition='discarded' if drop else 'forward')
            if drop:
                return
            self.send_response(response.status)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(body)))
            self.send_header('Connection', 'close')
            self.end_headers()
            self.wfile.write(body)
        except Exception as error:
            # No retry, even on timeout or journal failure. A response record
            # does not prove the downstream client received its bytes.
            try:
                self.server.journal.record(stage='failure', exchange=exchange,
                                           error_type=type(error).__name__)
            except Exception:
                pass
        finally:
            if connection is not None:
                connection.close()

    def log_message(self, *args):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--upstream', required=True)
    parser.add_argument('--listen-port', type=int, default=18791)
    parser.add_argument('--journal', required=True, type=Path)
    parser.add_argument('--run-id', required=True, type=uuid.UUID)
    parser.add_argument('--drop-post-responses', action='store_true')
    parser.add_argument('--serve', action='store_true')
    args = parser.parse_args()
    upstream = upstream_origin(args.upstream)
    if not 1 <= args.listen_port <= 65535:
        parser.error('listen port outside range')
    if not args.serve:
        print(json.dumps({'started': False, 'listen': ['127.0.0.1', args.listen_port],
                          'upstream': list(upstream), 'run_id': str(args.run_id),
                          'journal': str(args.journal),
                          'drop_post_responses': args.drop_post_responses}))
        return
    journal = Journal(args.journal, str(args.run_id), upstream, args.drop_post_responses)
    try:
        with Proxy(args.listen_port, upstream, journal, args.drop_post_responses) as proxy:
            try:
                proxy.serve_forever()
            except KeyboardInterrupt:
                pass
    finally:
        journal.close()


if __name__ == '__main__':
    main()
