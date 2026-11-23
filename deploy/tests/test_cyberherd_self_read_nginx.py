"""Actual isolated TLS/nginx self-only transport; harmless backend, no auth claim."""
import unittest
import test_nginx
from test_cyberherd_authenticated_nginx import transport_header


class SelfReadNginx(unittest.TestCase):
    CYBERHERD_SELF_READ_ENABLED = True
    setUpClass = classmethod(test_nginx.NginxAssemblyTests.setUpClass.__func__)
    stop_nginx = classmethod(test_nginx.NginxAssemblyTests.stop_nginx.__func__)
    request = test_nginx.NginxAssemblyTests.request

    def test_only_exact_self_read_preserves_authorization_origin_and_query(self):
        path = '/api/v1/cyberherd/me?unexpected=1'
        auth = transport_header(path, 'GET')
        status, headers, _ = self.request('production', path, headers={
            'Authorization': auth, 'Origin': 'https://lightning-goats.com'}, source='127.0.5.1')
        self.assertEqual(status, 200)
        self.assertEqual(headers['Cache-Control'], 'no-store')
        method, observed, forwarded = self.backends['production'].calls[-1]
        self.assertEqual((method, observed), ('GET', path))
        self.assertEqual(forwarded['Authorization'], auth)
        self.assertEqual(forwarded['Origin'], 'https://lightning-goats.com')
        for path in self.root.rglob('*.log'):
            self.assertNotIn(auth, path.read_text(errors='replace'))

    def test_operator_routes_methods_and_canary_never_reach_backend(self):
        before = {name: len(backend.calls) for name, backend in self.backends.items()}
        for index, method in enumerate(('HEAD', 'POST', 'PUT', 'PATCH', 'DELETE', 'OPTIONS')):
            self.assertEqual(self.request('production', '/api/v1/cyberherd/me', method,
                source=f'127.0.5.{index+10}')[0], 405)
        for path in ('/api/v1/cyberherd/operator/state', '/api/v1/cyberherd/operator/templates',
                     '/api/v1/cyberherd/operator/command', '/api/v1/cyberherd/me/extra'):
            self.assertEqual(self.request('production', path)[0], 404)
        self.assertEqual(self.request('canary', '/api/v1/cyberherd/me')[0], 404)
        self.assertEqual({name: len(backend.calls) for name, backend in self.backends.items()}, before)

    def test_oversize_get_body_is_refused_before_native_upstream(self):
        before = len(self.backends['production'].calls)
        self.assertEqual(self.request('production', '/api/v1/cyberherd/me', 'GET', b'x'*1025,
            source='127.0.5.20')[0], 413)
        self.assertEqual(len(self.backends['production'].calls), before)
