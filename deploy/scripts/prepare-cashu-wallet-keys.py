#!/usr/bin/env python3
"""Stage the first Cashu wallet seed and receiving key, then install the reviewed pair."""
import sys
sys.dont_write_bytecode = True
if 'verified_tool_identity' not in globals():
    if __name__ == '__main__':
        raise SystemExit('use the externally verified root-owned tool launcher')

    def verified_tool_identity():
        raise ValueError('verified tool launcher required')

import argparse
from contextlib import ExitStack
import grp
import hashlib
import hmac
import importlib.util
import json
import os
from pathlib import Path
import pwd
import re
import resource
import stat
import tempfile


SPEC = importlib.util.spec_from_file_location(
    'cashu_credential_review', Path(__file__).with_name('prepare-cashu-signer-client.py'))
CLIENT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CLIENT)
C, TX, FACTORY = CLIENT.C, CLIENT.TX, CLIENT.FACTORY
C.verified_tool_identity = lambda: verified_tool_identity()
STATE = Path('/var/lib/lightning-goats-cashu')
WALLET_USER = 'lightning-goats-cashu'
WALLET_GROUP = 'lg-cashu-ipc'
HOST_KEY_APP_ID = 'd3acecba0dad4cdfb8c9381528936c58'
KEY_BUNDLE = 'cashu-wallet-initial-keys'
STATE_FILES = ('wallet-seed.bin', 'receiving-key.bin')
NAK_ENV = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C', 'NO_COLOR': '1'}


def wallet_identity():
    wallet = pwd.getpwnam(WALLET_USER)
    group = grp.getgrnam(WALLET_GROUP)
    if wallet.pw_uid == 0 or group.gr_gid == 0 or wallet.pw_gid != group.gr_gid:
        raise ValueError('dedicated Cashu wallet identity required')
    return wallet.pw_uid, group.gr_gid


def guard():
    CLIENT.bundle_guard()
    C.protected(STATE.parent, directory=True)
    if hashlib.sha256(FACTORY.NAK.read_bytes()).hexdigest() != FACTORY.DIGEST:
        raise ValueError('pinned nak digest mismatch')
    identity = FACTORY.run(['/usr/bin/systemd-id128', '--app-specific=' + HOST_KEY_APP_ID, 'machine-id'])
    if (not re.fullmatch(rb'[0-9a-f]{32}\n', identity)
            or stat.S_IMODE(CLIENT.HOST_KEY.stat().st_mode) != 0o400):
        raise ValueError('existing systemd credential host key required')
    with CLIENT.HOST_KEY.open('rb') as key_file:
        if key_file.read(16) != bytes.fromhex(identity.decode()):
            raise ValueError('systemd credential host key belongs to a different machine')
    return wallet_identity()


def check_access_attributes(fd):
    if any(name.startswith('system.posix_acl') or name == 'security.capability'
           for name in os.listxattr(fd)):
        raise ValueError('unexpected wallet key access attributes')


def read_private_file(fd, length, uid, gid=None):
    before = os.fstat(fd)
    if (not stat.S_ISREG(before.st_mode) or stat.S_IMODE(before.st_mode) != 0o600
            or before.st_uid != uid or (gid is not None and before.st_gid != gid)
            or before.st_nlink != 1 or before.st_size != length):
        raise ValueError('wallet key requires exact private mode, owner and length')
    check_access_attributes(fd)
    material = os.read(fd, length + 1)
    after = os.fstat(fd)
    if (len(material) != length or not any(material)
            or (before.st_ctime_ns, before.st_mtime_ns) != (after.st_ctime_ns, after.st_mtime_ns)):
        raise ValueError('wallet key is empty or changed during read')
    return material


def read_seed(path, uid):
    if not path.is_absolute() or '..' in path.parts or path.name != STATE_FILES[0]:
        raise ValueError('absolute operator seed path required')
    directory_flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
    secret_flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC
    with ExitStack() as close:
        parent = os.open('/', directory_flags)
        close.callback(os.close, parent)
        for part in path.parent.parts[1:]:
            parent = os.open(part, directory_flags, dir_fd=parent)
            close.callback(os.close, parent)
            info = os.fstat(parent)
            if info.st_uid not in (0, uid) or info.st_mode & 0o022:
                raise ValueError('operator seed ancestor has unsafe owner or permissions')
            check_access_attributes(parent)
        info = os.fstat(parent)
        if info.st_uid != uid or stat.S_IMODE(info.st_mode) != 0o700:
            raise ValueError('operator seed directory requires mode 0700 and operator ownership')
        fd = os.open(path.name, secret_flags, dir_fd=parent)
        close.callback(os.close, fd)
        return read_private_file(fd, 64, uid)


def same_host_credential(operation, data):
    args = ['/usr/bin/systemd-creds', '--no-pager', '--newline=no', '--name=' + KEY_BUNDLE]
    if operation == 'encrypt':
        args.append('--with-key=host')
    return FACTORY.run([*args, operation, '-', '-'], data=data)


def receiving_public(raw, cache):
    public = FACTORY.run([str(FACTORY.NAK), '--config-path', str(cache), 'key', 'public', '--with-parity'],
                         data=raw.hex().encode() + b'\n', environment=NAK_ENV).strip().decode()
    if not re.fullmatch('0[23][0-9a-f]{64}', public):
        raise ValueError('invalid compressed Cashu receiving key')
    return public


def publish(source, target):
    # GNU mv's no-clobber publication is also used for inactive Cashu restores.
    FACTORY.run(['/usr/bin/mv', '-nT', '--', str(source), str(target)])
    if os.path.lexists(source):
        raise ValueError('wallet key destination appeared during publication')


def write_file(path, data, identity=None):
    with path.open('xb') as output:
        output.write(data)
        if identity is not None:
            os.fchown(output.fileno(), *identity)
        os.fchmod(output.fileno(), 0o600)
        output.flush()
        os.fsync(output.fileno())


def retain_record(workspace, target, value):
    pending = workspace / 'record'
    write_file(pending, (json.dumps(value, sort_keys=True, indent=2) + '\n').encode())
    publish(pending, target)
    TX.sync_directory(target.parent)


def verified_record(request, envelope, workspace):
    digest = CLIENT.encrypted_digest(envelope)
    material = same_host_credential('decrypt', envelope.read_bytes())
    if len(material) != 96:
        raise ValueError('invalid staged wallet key bundle')
    public = receiving_public(material[64:], workspace / 'nak')
    if public[2:] in {request['project_pubkey'], *request['reserved_pubkeys']}:
        raise ValueError('Cashu receiving key overlaps a reserved role')
    return dict(request, receiving_key=public, encrypted_sha256=digest), material


def prepare(directory, source, operator_name, project, reserved, approved):
    C.root()
    if not approved or not all(CLIENT.pubkey(value) for value in (project, *reserved)):
        raise ValueError('public bindings and explicit wallet key preparation are required')
    identity = guard()
    operator = pwd.getpwnam(operator_name)
    if operator.pw_uid in (0, identity[0]):
        raise ValueError('separate non-root seed operator required')
    CLIENT.stage_path(directory, create=True)
    request_path, envelope, review = (directory / name for name in ('REQUEST.json', 'wallet-keys.encrypted', 'REVIEW.json'))
    request = {'version': 1, 'tool': verified_tool_identity(), 'operator_uid': operator.pw_uid,
               'state': str(STATE), 'wallet_identity': list(identity), 'project_pubkey': project,
               'reserved_pubkeys': sorted(set(reserved) - {project})}
    with TX.locked(directory), tempfile.TemporaryDirectory(prefix='.cashu-keys-', dir=directory) as temporary:
        workspace = Path(temporary)
        if guard() != identity or os.path.lexists(STATE):
            raise ValueError('first-time wallet key preparation requires an absent runtime state')
        if os.path.lexists(request_path):
            if C.read_json(request_path) != request:
                raise ValueError('review directory already binds another wallet key request')
        else:
            if os.path.lexists(envelope) or os.path.lexists(review):
                raise ValueError('retained wallet keys require their original request')
            retain_record(workspace, request_path, request)
        seed = read_seed(source, operator.pw_uid)
        complete = os.path.lexists(review)
        prior = C.read_json(review) if complete else None
        if complete and (not isinstance(prior, dict) or CLIENT.encrypted_digest(envelope) != prior.get('encrypted_sha256')):
            raise ValueError('reviewed wallet key ciphertext differs')
        created = not os.path.lexists(envelope)
        if created:
            secret = FACTORY.run([str(FACTORY.NAK), '--config-path', str(workspace / 'nak'), 'key', 'generate'],
                                 environment=NAK_ENV).strip()
            if not re.fullmatch(rb'[0-9a-f]{64}', secret):
                raise ValueError('nak generated an invalid Cashu receiving key')
            material = seed + bytes.fromhex(secret.decode())
            pending = workspace / 'encrypted'
            write_file(pending, same_host_credential('encrypt', material))
            record, checked = verified_record(request, pending, workspace)
            if not hmac.compare_digest(checked, material):
                raise ValueError('wallet key readback differs')
            publish(pending, envelope)
            TX.sync_directory(directory)
        else:
            record, checked = verified_record(request, envelope, workspace)
        if not hmac.compare_digest(seed, checked[:64]):
            raise ValueError('operator seed differs from the retained wallet key review')
        if complete:
            if prior != record:
                raise ValueError('sealed wallet key review differs')
        else:
            retain_record(workspace, review, record)
        if guard() != identity:
            raise ValueError('wallet runtime identity changed')
        return {'receiving_key': record['receiving_key'], 'keys_staged': created,
                'review_sha256': hashlib.sha256(review.read_bytes()).hexdigest()}


def verify_installation(identity, material):
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
    with ExitStack() as close:
        descriptor = os.open(STATE, flags)
        close.callback(os.close, descriptor)
        info = os.fstat(descriptor)
        check_access_attributes(descriptor)
        if ((info.st_uid, info.st_gid) != identity or stat.S_IMODE(info.st_mode) != 0o700
                or set(os.listdir(descriptor)) != set(STATE_FILES)):
            raise ValueError('first-time wallet directory differs from the reviewed key pair')
        for name, data in zip(STATE_FILES, (material[:64], material[64:]), strict=True):
            fd = os.open(name, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=descriptor)
            close.callback(os.close, fd)
            if not hmac.compare_digest(read_private_file(fd, len(data), *identity), data):
                raise ValueError('installed wallet keys differ from the reviewed pair')


def install(directory, expected_review, approved):
    C.root()
    if not approved or not re.fullmatch('[0-9a-f]{64}', expected_review):
        raise ValueError('explicit wallet key installation and reviewed digest required')
    identity = guard()
    CLIENT.stage_path(directory)
    with TX.locked(directory), tempfile.TemporaryDirectory(prefix='.cashu-keys-', dir=directory) as temporary:
        review = directory / 'REVIEW.json'
        record, request = C.read_json(review), C.read_json(directory / 'REQUEST.json')
        envelope = directory / 'wallet-keys.encrypted'
        if (not isinstance(record, dict) or not isinstance(request, dict)
                or hashlib.sha256(review.read_bytes()).hexdigest() != expected_review
                or CLIENT.encrypted_digest(envelope) != record.get('encrypted_sha256')
                or request.get('tool') != verified_tool_identity()
                or request.get('state') != str(STATE) or request.get('wallet_identity') != list(identity)):
            raise ValueError('reviewed wallet key binding differs')
        verified, material = verified_record(request, envelope, Path(temporary))
        if record != verified:
            raise ValueError('reviewed Cashu receiving identity differs')
        created = not os.path.lexists(STATE)
        if created:
            with tempfile.TemporaryDirectory(prefix='.cashu-wallet-key-install-', dir=STATE.parent) as installing:
                pending = Path(installing) / 'state'
                pending.mkdir(mode=0o700)
                for name, data in zip(STATE_FILES, (material[:64], material[64:]), strict=True):
                    write_file(pending / name, data, identity)
                os.chown(pending, *identity)
                TX.sync_directory(pending)
                if guard() != identity:
                    raise ValueError('wallet runtime identity changed')
                publish(pending, STATE)
        verify_installation(identity, material)
        if Path('/sys/fs/selinux/enforce').exists():
            FACTORY.run(['/usr/sbin/restorecon', '-R', str(STATE)])
        TX.sync_directory(STATE)
        TX.sync_directory(STATE.parent)
        if guard() != identity:
            raise ValueError('wallet runtime identity changed')
        return {'receiving_key': record['receiving_key'], 'keys_installed': True,
                'state_created': created, 'journal_initialized': False, 'services_started': False}


def main():
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    parser = argparse.ArgumentParser(description=__doc__)
    operations = parser.add_subparsers(dest='operation', required=True)
    prepare_parser = operations.add_parser('prepare')
    prepare_parser.add_argument('directory', type=Path)
    prepare_parser.add_argument('--seed-source', type=Path, required=True)
    prepare_parser.add_argument('--seed-user', required=True)
    prepare_parser.add_argument('--project-pubkey', required=True)
    prepare_parser.add_argument('--reserved-pubkey', action='append', default=[])
    prepare_parser.add_argument('--prepare-wallet-keys', action='store_true', required=True)
    install_parser = operations.add_parser('install')
    install_parser.add_argument('directory', type=Path)
    install_parser.add_argument('--review-sha256', required=True)
    install_parser.add_argument('--install-reviewed-wallet-keys', action='store_true', required=True)
    args = parser.parse_args()
    if args.operation == 'prepare':
        result = prepare(args.directory, args.seed_source, args.seed_user, args.project_pubkey,
                         args.reserved_pubkey, args.prepare_wallet_keys)
    else:
        result = install(args.directory, args.review_sha256, args.install_reviewed_wallet_keys)
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
