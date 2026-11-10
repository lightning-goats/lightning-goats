#!/usr/bin/env python3
"""Prepare two distinct NIP-46 authentication clients, never a project key.

Authenticated root launcher only. No bunker authorization, signing, publication
or service change. Uses the existing pinned nak/encrypted-client factory.
"""
import sys
sys.dont_write_bytecode = True
if 'verified_tool_identity' not in globals():
    if __name__ == '__main__':
        raise SystemExit('use the externally verified root-owned tool launcher')

    def verified_tool_identity():
        raise ValueError('verified tool launcher required')

import argparse
import importlib.util
import json
from pathlib import Path
import re


def helper(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


C = helper('cyberherd_client_support', 'upgrade-cyberherd-core.py')
C.verified_tool_identity = lambda: verified_tool_identity()
FACTORY = helper('cyberherd_client_factory', 'prepare-household-nostr-client.py')
HOST_KEY = Path('/var/lib/systemd/credential.secret')


def prepare(project, core_client, approved):
    C.root()
    if not approved:
        raise ValueError('explicit new-client preparation flag required')
    if any(re.fullmatch('[0-9a-f]{64}', value) is None for value in (project, core_client)) or project == core_client:
        raise ValueError('distinct actual project/core-client public identities required')
    C.protected(FACTORY.STORE, directory=True, private=True)
    C.protected(FACTORY.NAK)
    C.protected(HOST_KEY, private=True)
    clients = {}
    for role, filename in [('cyberherd', 'cyberherd-nostr-client'), ('cyberherd-signer', 'cyberherd-signer-client')]:
        result = FACTORY.prepare(FACTORY.STORE / filename)
        if result['client_public_key'] in {project, core_client, *(row['client_public_key'] for row in clients.values())}:
            raise ValueError('CyberHerd authentication clients must be separate from project/core/each other; no automatic rotation')
        C.protected(Path(result['credential']), private=True)
        clients[role] = result
    return {'clients': clients, 'household_changed': False, 'services_changed': False, 'events_published': 0}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--project-pubkey', required=True)
    parser.add_argument('--core-client-pubkey', required=True)
    parser.add_argument('--create-client-keys', action='store_true', required=True)
    args = parser.parse_args()
    print(json.dumps(prepare(args.project_pubkey, args.core_client_pubkey, args.create_client_keys), sort_keys=True))


if __name__ == '__main__':
    main()
