#!/usr/bin/python3
"""Synthetic process-contract fixture, never cryptographic acceptance."""
import base64
import json
import os
from pathlib import Path
import sys
args = sys.argv[1:]
assert args[0] == '--config-path'
root = Path(args[1]); args = args[2:]
mode = (root/'mode').read_text().strip()
raw = sys.stdin.read().strip()
payload = json.loads(raw)
phase = 'wrap' if args[0] == 'gift' else ('verify' if args[0] == 'verify' else 'publish')
with (root/'calls.jsonl').open('a') as output:
    output.write(json.dumps({'phase':phase,'args':args,'raw':raw,
        'client_key':bool(os.environ.get('NOSTR_CLIENT_KEY')),
        'signer':bool(os.environ.get('NOSTR_SECRET_KEY'))})+'\n')
if mode == 'no_encryption' and phase == 'wrap':
    sys.stderr.write('SENSITIVE-PRIVATE-ERROR'); sys.exit(7)
if phase == 'verify':
    sys.exit(1 if mode == 'bad_signature' else 0)
if phase == 'publish':
    if mode == 'publish_failure': sys.exit(4)
    if mode == 'mutated_publish': payload['id'] = 'aa'*32
    print(json.dumps(payload)); sys.exit(0)
assert args[:4] == ['gift','wrap','--use-our-identity-key','--use-their-identity-key']
assert payload['kind'] == 14 and payload['tags'] == [['p', args[-1]]]
event = {'id':'01'*32,'pubkey':'cd'*32,'created_at':1700000000,'kind':1059,
         'tags':payload['tags'],'content':base64.b64encode(bytes([2])+bytes(98)).decode(),'sig':'02'*64}
if mode == 'wrong_recipient': event['tags'] = [['p','ef'*32]]
if mode == 'public_kind': event['kind'] = 1
if mode == 'plaintext': event['content'] = payload['content']
if mode == 'extra_field': event['plaintext'] = payload['content']
if mode == 'malformed': event['kind'] = 'SENSITIVE-PRIVATE-ERROR'
print(json.dumps(event))
