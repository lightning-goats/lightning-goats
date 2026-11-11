"""Exact operator additions alongside the existing self-read transport.

Harmless upstream mocks verify transport, not native authorization or live WAF.
"""
import unittest
import test_nginx
import test_cyberherd_authenticated_nginx as authenticated


class OperatorOnlyNginxTests(unittest.TestCase):
    CYBERHERD_SELF_READ_ENABLED = True
    CYBERHERD_OPERATOR_ONLY_ENABLED = True
    setUpClass = classmethod(test_nginx.NginxAssemblyTests.setUpClass.__func__)
    stop_nginx = classmethod(test_nginx.NginxAssemblyTests.stop_nginx.__func__)
    request = test_nginx.NginxAssemblyTests.request
    test_exact_read_paths_preserve_query_authorization_and_origin = authenticated.AuthenticatedCyberHerdNginxTests.test_exact_read_paths_preserve_query_authorization_and_origin
    test_command_preserves_exact_payload_and_does_not_log_private_fields = authenticated.AuthenticatedCyberHerdNginxTests.test_command_preserves_exact_payload_and_does_not_log_private_fields

    def test_methods_unknown_routes_and_oversize_commands_never_reach_upstream(self):
        before = len(self.backends['production'].calls)
        for i, path in enumerate(authenticated.READS):
            for j, method in enumerate(('HEAD', 'POST', 'PUT', 'DELETE', 'OPTIONS')):
                self.assertEqual(self.request('production', path, method, source=f'127.0.8.{i*10+j+1}')[0], 405)
        for j, method in enumerate(('GET', 'HEAD', 'PUT', 'DELETE', 'OPTIONS')):
            self.assertEqual(self.request('production', authenticated.COMMAND, method, source=f'127.0.9.{j+1}')[0], 405)
        self.assertEqual(self.request('production', authenticated.COMMAND, 'POST', b'x'*16385,
                                     {'Content-Type':'application/json'}, source='127.0.9.10')[0], 413)
        self.assertEqual(self.request('production', authenticated.COMMAND+'/extra')[0], 404)
        self.assertEqual(self.request('canary', authenticated.COMMAND)[0], 404)
        self.assertEqual(len(self.backends['production'].calls), before)
