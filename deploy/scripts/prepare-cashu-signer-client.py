#!/usr/bin/env python3
"""Prepare and separately install the dedicated Cashu NIP-46 client ciphertext."""
import sys
sys.dont_write_bytecode = True
if 'verified_tool_identity' not in globals():
    if __name__ == '__main__':
        raise SystemExit('use the externally verified root-owned tool launcher')

    def verified_tool_identity():
        raise ValueError('verified tool launcher required')

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import tempfile


def helper(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


P = helper('prepare-contact-upgrade')
C, TX = P.C, P.TX
C.verified_tool_identity = lambda: verified_tool_identity()
FACTORY = helper('prepare-household-nostr-client')
HOST_KEY = Path('/var/lib/systemd/credential.secret')
NAME = 'cashu-nostr-client'
DESTINATION = 'lg-cashu-nostr-client'
MAX_CREDENTIAL = 16_384


def pubkey(value):
    return isinstance(value, str) and re.fullmatch('[0-9a-f]{64}', value) is not None and value != '0' * 64


def stage_path(directory, *, create=False):
    C.protected(C.BASE, directory=True, private=True)
    if directory.parent != C.BASE or re.fullmatch('[A-Za-z0-9_.-]{1,128}', directory.name) is None:
        raise ValueError('private credential review must be a direct child of the upgrade directory')
    if create:
        directory.mkdir(mode=0o700, exist_ok=True)
        TX.sync_directory(C.BASE)
    C.protected(directory, directory=True, private=True)
    lock = directory / 'LOCK'
    if create:
        try:
            lock.touch(mode=0o600, exist_ok=False)
        except FileExistsError:
            pass
    C.protected(lock, private=True)


def bundle_guard():
    P.require_inactive_cashu()
    C.protected(HOST_KEY, private=True)
    C.protected(FACTORY.NAK)


def encrypted_digest(path):
    C.protected(path, private=True)
    if not 0 < path.stat().st_size <= MAX_CREDENTIAL or path.stat().st_mode & 0o7777 != 0o600:
        raise ValueError('invalid protected Cashu ciphertext')
    return hashlib.sha256(path.read_bytes()).hexdigest()


def prepare(directory, project, reserved, approved):
    C.root()
    if not approved or not all(pubkey(value) for value in (project, *reserved)):
        raise ValueError('valid public bindings and explicit credential preparation are required')
    bundle_guard()
    stage_path(directory, create=True)
    draft = directory / 'REQUEST.json'
    ciphertext = directory / 'cashu-client.encrypted'
    review = directory / 'REVIEW.json'
    request = {'version': 1, 'tool': verified_tool_identity(), 'project_pubkey': project,
               'reserved_pubkeys': sorted(set(reserved) - {project})}
    with TX.locked(directory):
        bundle_guard()
        if not os.path.lexists(draft):
            if os.path.lexists(ciphertext) or os.path.lexists(review):
                raise ValueError('retained ciphertext requires its original request')
            C.write_json(draft, request)
        elif C.read_json(draft) != request:
            raise ValueError('review directory already binds a different client request')
        complete = os.path.lexists(review)
        if complete:
            retained_review = C.read_json(review)
            if not isinstance(retained_review, dict) or encrypted_digest(ciphertext) != retained_review.get('encrypted_sha256'):
                raise ValueError('reviewed ciphertext differs')
        elif os.path.lexists(ciphertext):
            encrypted_digest(ciphertext)
        result = FACTORY.prepare(ciphertext, credential_name=NAME,
                                 forbidden={project, *reserved}, create=not complete)
        record = dict(request, client_public_key=result['client_public_key'],
                      encrypted_sha256=encrypted_digest(ciphertext))
        if complete:
            if retained_review != record:
                raise ValueError('sealed review differs from the retained credential')
        else:
            C.write_json(review, record)
        bundle_guard()
        return {'client_public_key': record['client_public_key'],
                'review_sha256': hashlib.sha256(review.read_bytes()).hexdigest(),
                'credential_created': result['created'], 'installed': False}


def install(directory, expected_review, approved):
    C.root()
    if not approved or re.fullmatch('[0-9a-f]{64}', expected_review) is None:
        raise ValueError('explicit credential installation and reviewed digest are required')
    bundle_guard()
    stage_path(directory)
    C.protected(FACTORY.STORE, directory=True, private=True)
    target = FACTORY.STORE / DESTINATION
    ciphertext = directory / 'cashu-client.encrypted'
    review = directory / 'REVIEW.json'
    with TX.locked(directory):
        bundle_guard()
        record = C.read_json(review)
        if not isinstance(record, dict) or hashlib.sha256(review.read_bytes()).hexdigest() != expected_review:
            raise ValueError('credential review digest differs')
        request = C.read_json(directory / 'REQUEST.json')
        project, reserved = request['project_pubkey'], request['reserved_pubkeys']
        if (request != {'version': 1, 'tool': verified_tool_identity(), 'project_pubkey': project, 'reserved_pubkeys': reserved}
                or not isinstance(reserved, list) or not all(pubkey(value) for value in (project, *reserved))):
            raise ValueError('credential request binding differs')
        digest = encrypted_digest(ciphertext)
        if digest != record.get('encrypted_sha256'):
            raise ValueError('reviewed ciphertext differs')
        result = FACTORY.prepare(ciphertext, credential_name=NAME, forbidden={project, *reserved}, create=False)
        if record != dict(request, client_public_key=result['client_public_key'], encrypted_sha256=digest):
            raise ValueError('reviewed client identity or ciphertext differs')
        created = not os.path.lexists(target)
        if created:
            with tempfile.TemporaryDirectory(dir=FACTORY.STORE, prefix='.cashu-client-install-') as temporary:
                pending = Path(temporary) / 'ciphertext'
                with pending.open('xb') as output, ciphertext.open('rb') as source:
                    shutil.copyfileobj(source, output)
                    output.flush()
                    os.fsync(output.fileno())
                if encrypted_digest(pending) != digest:
                    raise ValueError('copied ciphertext differs')
                bundle_guard()
                os.link(pending, target)
        if encrypted_digest(target) != digest:
            raise ValueError('installed credential differs; preserve it for review')
        if Path('/sys/fs/selinux/enforce').exists():
            FACTORY.run(['/usr/sbin/restorecon', str(target)])
        with target.open('rb') as installed:
            os.fsync(installed.fileno())
        TX.sync_directory(FACTORY.STORE)
        bundle_guard()
        return {'client_public_key': record['client_public_key'], 'installed': True,
                'credential_created': created, 'services_started': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    operations = parser.add_subparsers(dest='operation', required=True)
    plan = operations.add_parser('prepare')
    plan.add_argument('directory', type=Path)
    plan.add_argument('--project-pubkey', required=True)
    plan.add_argument('--reserved-pubkey', action='append', default=[])
    plan.add_argument('--prepare-client-key', action='store_true', required=True)
    apply = operations.add_parser('install')
    apply.add_argument('directory', type=Path)
    apply.add_argument('--review-sha256', required=True)
    apply.add_argument('--install-reviewed-key', action='store_true', required=True)
    args = parser.parse_args()
    if args.operation == 'prepare':
        result = prepare(args.directory, args.project_pubkey, args.reserved_pubkey, args.prepare_client_key)
    else:
        result = install(args.directory, args.review_sha256, args.install_reviewed_key)
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
