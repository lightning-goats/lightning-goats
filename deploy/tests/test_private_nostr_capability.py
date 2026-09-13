"""Pinned real NIP-46 encryption/signing proof; synthetic identities only.

Requires LG_TEST_NAK and a fresh loopback-only network namespace. Never invoke
with project credentials. The production wrapper receives public test scalar 01.
"""
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
PIN = 'b44b36c792fbc3fb73b7ba3bbc94beda2219826271aa8d5f130f569c3817c3b9'
SIGNER = '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798'
CLIENT = 'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5'
RECIPIENT = 'f9308a019258c31049344f85f89d5229b531c845836f99b08601f113bce036f9'
NAK = os.environ.get('LG_TEST_NAK')


@unittest.skipUnless(NAK, 'requires pinned nak and isolated loopback namespace')
class PrivateNostrCapability(unittest.TestCase):
    def test_real_bunker_encrypts_and_signs_without_message_publication(self):
        links = json.loads(subprocess.check_output(['ip', '-j', 'link']))
        self.assertEqual([link['ifname'] for link in links], ['lo'])
        nak = Path(NAK).resolve()
        self.assertEqual(hashlib.sha256(nak.read_bytes()).hexdigest(), PIN)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ('relay', 'client', 'recipient', 'credentials', 'bunker'):
                (root / name).mkdir(mode=0o700)
            key = root / 'credentials/nostr-key'
            key.write_text('01\n')  # Public test vector, not a project secret.
            key.chmod(0o600)
            with socket.socket() as sock:
                sock.bind(('127.0.0.1', 0))
                port = sock.getsockname()[1]
            url = f'ws://127.0.0.1:{port}'
            processes = []
            def start(args, env):
                child = subprocess.Popen(args, env=env, stdin=subprocess.DEVNULL,
                                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                processes.append(child)
                return child
            def run(args, body='', signer=None):
                env = {'NO_COLOR': '1'}
                if signer:
                    env['NOSTR_SECRET_KEY'], env['NOSTR_CLIENT_KEY'] = signer
                result = subprocess.run([str(nak), '--config-path', str(root / 'client'), *args],
                                        input=body+'\n', text=True, capture_output=True,
                                        env=env, timeout=45)
                # Never echo subprocess diagnostic/body into test failure logs.
                self.assertEqual(result.returncode, 0, 'isolated nak command failed')
                self.assertLessEqual(len(result.stdout.encode()), 65536)
                return result.stdout.strip()
            try:
                relay = start([str(nak), '--config-path', str(root/'relay'), 'serve',
                               '--hostname', '127.0.0.1', '--port', str(port)], {'NO_COLOR':'1'})
                deadline = time.monotonic()+10
                while True:
                    self.assertIsNone(relay.poll(), 'isolated relay exited')
                    try:
                        with socket.create_connection(('127.0.0.1', port), timeout=.1):
                            break
                    except OSError:
                        self.assertLess(time.monotonic(), deadline, 'relay startup deadline')
                        time.sleep(.02)
                bunker = start(['/bin/sh', str(ROOT/'deploy/scripts/run-nak-bunker')],
                               {'NO_COLOR':'1', 'NAK_BIN':str(nak),
                                'CREDENTIALS_DIRECTORY':str(root/'credentials'),
                                'RUNTIME_DIRECTORY':str(root/'bunker'),
                                'LG_NOSTR_CLIENT_PUBKEY':CLIENT, 'LG_NOSTR_RELAYS':url})
                time.sleep(.3)
                self.assertIsNone(bunker.poll(), 'bunker wrapper exited')
                message = 'Synthetic private sweep-alert capability test only.'
                rumor = {'kind':14, 'created_at':int(time.time()), 'tags':[['p',RECIPIENT]],
                         'content':message}
                raw = run(['gift','wrap','--use-our-identity-key','--use-their-identity-key',
                           '-p',RECIPIENT], json.dumps(rumor),
                          (f'bunker://{SIGNER}?relay={url}', '02'))
                wrap = json.loads(raw)
                self.assertEqual(wrap['kind'], 1059)
                self.assertEqual(wrap['tags'], [['p',RECIPIENT]])
                self.assertNotEqual(wrap['pubkey'], SIGNER)
                self.assertNotIn(message, raw)
                run(['verify'], raw)
                # Explicit decrypt avoids gift unwrap's public decoupled-key lookup.
                seal_raw = run(['decrypt','-p',wrap['pubkey'],wrap['content']], signer=('03',''))
                seal = json.loads(seal_raw)
                self.assertEqual(seal['kind'], 13)
                self.assertEqual(seal['pubkey'], SIGNER)
                self.assertEqual(seal['tags'], [])
                run(['verify'], seal_raw)
                decoded = json.loads(run(['decrypt','-p',SIGNER,seal['content']], signer=('03','')))
                self.assertEqual(decoded['kind'], 14)
                self.assertEqual(decoded['pubkey'], SIGNER)
                self.assertEqual(decoded['content'], message)
                self.assertEqual(decoded['tags'], rumor['tags'])
                self.assertEqual(decoded['created_at'], rumor['created_at'])
                canonical = [0,SIGNER,decoded['created_at'],14,decoded['tags'],message]
                self.assertEqual(decoded['id'], hashlib.sha256(json.dumps(canonical,
                                 separators=(',',':'),ensure_ascii=False).encode()).hexdigest())
                self.assertTrue(not decoded.get('sig') or set(decoded['sig']) == {'0'})
                self.assertEqual(run(['req','--kind','1','--kind','14','--kind','13',
                                      '--kind','1059',url]), '', 'wrapping published a message')
            finally:
                for process in reversed(processes):
                    if process.poll() is None:
                        process.kill()
                    process.wait(timeout=5)


if __name__ == '__main__':
    unittest.main()
