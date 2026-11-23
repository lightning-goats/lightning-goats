import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("retire", Path(__file__).parents[1] / "scripts/retire-lnbits-ingress.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class RetireTests(unittest.TestCase):
    def test_only_legacy_locations_change_including_nested_acl(self):
        source = '''server {
 location / { try_files $uri =404; }
 location ^~ /lnurlp/ {
  limit_except GET { deny all; }
  proxy_pass http://10.8.0.6:3002;
 }
 location = /.well-known/nostr.json {
  proxy_pass http://10.8.0.6:3002/nostrnip5/public;
 }
 location /other/ { proxy_pass http://127.0.0.1:8088; }
}
'''
        result, count = MODULE.retire(source)
        self.assertEqual(count, 2)
        self.assertIn("location / { try_files $uri =404; }", result)
        self.assertIn("location /other/ { proxy_pass http://127.0.0.1:8088; }", result)
        self.assertNotIn("3002", result)
        self.assertEqual(result.count("return 410"), 2)

    def test_unknown_backend_reference_fails_without_partial_acceptance(self):
        with self.assertRaisesRegex(ValueError, "unhandled"):
            MODULE.retire("upstream legacy { server 10.8.0.6:3002; }")
