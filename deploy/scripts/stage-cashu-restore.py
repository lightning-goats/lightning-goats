#!/usr/bin/env python3
"""Verify a collected checkpoint and stage fenced database copies for review."""
import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import re
import resource
import runpy
import sqlite3
import subprocess
import tarfile
import time
import tomllib

HERE = Path(__file__).resolve().parent


def relative_path(name):
    path = Path(name)
    if (not path.parts or path.is_absolute() or str(path) != name
            or len(path.parts) > 12
            or any(not re.fullmatch(r'[A-Za-z0-9_.-]+', part)
                   or part in ('.', '..') for part in path.parts)):
        raise ValueError('invalid checkpoint member name')
    return path


def extract_plain_tar(archive, destination, budget):
    """Accept only the collector's USTAR regular files and directories."""
    destination.mkdir(mode=0o700)
    seen = set()
    block = tarfile.BLOCKSIZE
    while True:
        budget.remaining()
        header = archive.read(block)
        if header == bytes(block):
            if archive.read(block) != bytes(block):
                raise ValueError('invalid checkpoint archive terminator')
            while chunk := archive.read(65536):
                budget.remaining()
                if any(chunk):
                    raise ValueError('data follows the checkpoint terminator')
            break
        if (len(header) != block or header[257:265] != b'ustar\00000'
                or header[156:157] not in (b'0', b'5')):
            raise ValueError('unsupported checkpoint tar header')
        member = tarfile.TarInfo.frombuf(header, 'ascii', 'strict')
        relative = relative_path(member.name)
        if relative in seen or len(seen) >= 4096:
            raise ValueError('duplicate or excessive checkpoint members')
        seen.add(relative)
        output = destination / relative
        if not output.parent.is_dir():
            raise ValueError('checkpoint directory is missing or out of order')
        if member.isdir():
            if member.size != 0:
                raise ValueError('checkpoint directory carries data')
            output.mkdir(mode=0o700)
            continue
        if not 0 <= member.size <= 1024**3:
            raise ValueError('checkpoint member exceeds bound')
        budget.add(member.size)
        with output.open('xb') as target:
            os.fchmod(target.fileno(), 0o600)
            remaining = member.size
            while remaining:
                budget.remaining()
                chunk = archive.read(min(65536, remaining))
                if not chunk:
                    raise ValueError('checkpoint member was truncated')
                target.write(chunk)
                remaining -= len(chunk)
            target.flush()
            os.fsync(target.fileno())
        padding = (-member.size) % block
        if archive.read(padding) != bytes(padding):
            raise ValueError('invalid checkpoint member padding')
    if not seen:
        raise ValueError('checkpoint archive is empty')


def read_identity(identity_file):
    identity_file.seek(0)
    lines = identity_file.read(4097).splitlines()
    # Keep age in its native X25519 mode. Other identity formats may start
    # helper executables or prompt for passphrases.
    keys = [line for line in lines if line and not line.startswith(b'#')]
    if len(keys) != 1 or not re.fullmatch(rb'AGE-SECRET-KEY-1[023456789ACDEFGHJKLMNPQRSTUVWXYZ]{58}', keys[0]):
        raise ValueError('one native age X25519 identity is required')
    identity_file.seek(0)


def decrypt(archive, identity, output, age, budget):
    def limit_output():
        resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
        resource.setrlimit(resource.RLIMIT_FSIZE, (budget.limit, budget.limit))
    read_identity(identity)
    archive.seek(0)
    with output.open('xb') as target:
        os.fchmod(target.fileno(), 0o600)
        result = subprocess.run(
            [str(age), '--decrypt', '--identity', f'/proc/self/fd/{identity.fileno()}'],
            stdin=archive, stdout=target, stderr=subprocess.DEVNULL,
            pass_fds=(identity.fileno(),), timeout=budget.remaining(300),
            env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}, preexec_fn=limit_output,
        )
        if result.returncode or not 0 < target.tell() <= budget.limit:
            raise ValueError('age authentication or decryption failed')
        target.flush()
        os.fsync(target.fileno())
        budget.add(target.tell())


def verify_copies(plain, collection, common, budget):
    records = collection['originals']
    if not isinstance(records, list) or not 1 <= len(records) <= 4096:
        raise ValueError('invalid checkpoint copy inventory')
    checked = set()
    for record in records:
        name = relative_path(record['path'])
        if name in checked:
            raise ValueError('duplicate recorded checkpoint file')
        checked.add(name)
        with closing(common['Frozen'](plain / name, 0, 1024**3, budget, empty=True)) as copy:
            if (copy.digest != record['sha256'] or copy.expected[6] != record['bytes']):
                raise ValueError('recorded checkpoint file differs')


def image_plan(checkpoint, inventory, native):
    plan = {role: [] for role in ('core', 'membership', 'payout', 'signer', 'wallet', 'cashu_signer')}
    for role in ('core', 'membership', 'payout', 'signer'):
        name = Path(native['policy']['components'][role]['database']).name
        target = relative_path(name)
        if len(target.parts) != 1 or target.name.endswith(('-wal', '-shm', '-journal')):
            raise ValueError('invalid native database basename')
        source = checkpoint / 'native' / (role + '.db')
        plan[role].append((source, target))
    names = {'wallet_journal': ('wallet', 'acquisition.sqlite'),
             'cdk_wallet': ('wallet', 'wallet.sqlite'),
             'signing_broker': ('cashu_signer', 'cashu-signing.sqlite'),
             'wallet_seed': ('wallet', 'wallet-seed.bin'),
             'receiving_key': ('wallet', 'receiving-key.bin')}
    for row in inventory['components']:
        if row['role'] == 'cdk_mint_wallet':
            mint = row['mint_id'].encode()
            namespace = hashlib.sha256(b'lightning-goats/cdk-mint-store/v1\0' + mint).hexdigest()
            role, name = 'wallet', 'mint-' + namespace + '.sqlite'
        elif row['role'] in names:
            role, name = names[row['role']]
        else:
            continue
        plan[role].append((checkpoint / 'cashu' / relative_path(row['path']), Path(name)))
    return plan


def verify_key_bindings(plain, checkpoint, inventory, native, policy, common, budget):
    read = common['COMBINED']['read_private']
    document = common['document']
    keys = plain / 'recovery-material'
    bindings = document(read(keys / 'key-bindings.json', 65536))
    wallet = document(read(plain / 'wallet-config', 65536))
    cashu_signer = document(read(plain / 'cashu_signer-config', 65536))
    payout_signer = tomllib.loads(read(plain / 'signer-config', 65536).decode())
    by_role = {row['role']: row for row in inventory['components']}
    journal = checkpoint / 'cashu' / relative_path(by_role['wallet_journal']['path'])
    with closing(sqlite3.connect(journal.as_uri() + '?mode=ro&immutable=1', uri=True)) as db:
        db.execute('PRAGMA trusted_schema=OFF')
        db.set_progress_handler(lambda: int(time.monotonic() >= budget.deadline), 10000)
        rows = db.execute('SELECT instance,key_binding FROM runtime_registration WHERE singleton=1').fetchall()
    if len(rows) != 1 or rows[0][0] != inventory['registrations']['wallet_instance']:
        raise ValueError('wallet registration differs from copied key binding')
    anchor, configured_keys = common['receiving_material'](wallet)
    historical = [{'name': f'historical-{index}.bin', 'public_key': key['public_key']}
                  for index, key in enumerate(policy['historical_receiving_keys'])]
    historical.extend({'name': name, 'public_key': public_key}
                      for name, public_key in configured_keys.items())
    expected = {
        'version': 1, 'owner': native['policy']['owner'], 'receiving_key': anchor,
        'wallet_key_binding': rows[0][1].hex(), 'cashu_client': cashu_signer['nak']['client_pubkey'],
        'payout_client': payout_signer['policy']['client_pubkey'],
        'historical': historical,
    }
    if bindings != expected:
        raise ValueError('archived recovery-key bindings differ from checkpoint scope')
    for role, filename in (('wallet_seed', 'wallet-seed.bin'), ('receiving_key', 'receiving-key.bin')):
        if hashlib.sha256(read(keys / filename, 64)).hexdigest() != by_role[role]['sha256']:
            raise ValueError('collected recovery key differs from wallet image inventory')
    return configured_keys


def stage(args, common, tool_identity):
    if os.geteuid() != 0:
        raise ValueError('root restore review required')
    if not 4096 <= args.max_bytes <= 64 * 1024**3 or not 30 <= args.deadline_seconds <= 1800:
        raise ValueError('invalid restore byte or duration bound')
    if not re.fullmatch('[0-9a-f]{64}', args.archive_sha256):
        raise ValueError('independently retained ciphertext digest required')
    budget = common['Budget'](args.max_bytes, args.deadline_seconds)
    destination = args.destination
    common['protected_directory'](destination.parent, private=True)
    if not destination.is_absolute() or destination.name in ('', '.', '..'):
        raise ValueError('new absolute checkpoint stage required')
    read = common['COMBINED']['read_private']
    document = common['document']
    with closing(common['Frozen'](args.receipt, 0, 65536, budget)) as receipt_file:
        receipt_file.file.seek(0)
        receipt = document(receipt_file.file.read())
        receipt_file.check()
    if (receipt['result'] != 'CASHU_COORDINATED_BACKUP_COLLECTED'
            or type(receipt['version']) is not int or receipt['version'] != 1
            or receipt['ciphertext_sha256'] != args.archive_sha256
            or receipt['quiescence_verified'] is not True or receipt['restore_authorized'] is not False
            or receipt['resume_authorized'] is not False):
        raise ValueError('collection receipt does not bind this ciphertext')

    with (closing(common['Frozen'](args.archive, 0, args.max_bytes, budget)) as archive,
          closing(common['Frozen'](args.identity, 0, 4096, budget)) as identity,
          closing(common['Frozen'](args.age, 0, 512 * 1024**2, budget, secret=False)) as age):
        if (archive.digest != args.archive_sha256 or archive.expected[6] != receipt['ciphertext_bytes']
                or age.digest != args.age_sha256 or not age.expected[4] & 0o100):
            raise ValueError('selected ciphertext or age executable differs')
        destination.mkdir(mode=0o700)
        marker = destination / 'INCOMPLETE'
        common['write'](marker, b'CASHU_RESTORE_STAGE_V1\n')
        common['sync'](destination)
        common['sync'](destination.parent)
        temporary_tar = destination / 'decrypted.tar'
        decrypt(archive.file, identity.file, temporary_tar, age.path, budget)
        archive.check()
        identity.check()
        age.check()

    plain = destination / 'checkpoint-evidence'
    with temporary_tar.open('rb') as archive:
        extract_plain_tar(archive, plain, budget)
    checkpoint = plain / 'checkpoint'
    collection_raw = read(plain / 'collection.json', 4 * 1024**2)
    collection = document(collection_raw)
    policy_raw = read(plain / 'collection-policy.json', 65536)
    policy = document(policy_raw)
    if (collection['checkpoint_id'] != receipt['checkpoint_id']
            or collection['policy_sha256'] != receipt['policy_sha256']
            or common['sha'](policy_raw) != receipt['policy_sha256']
            or collection['restore_authorized'] is not False
            or collection['resume_authorized'] is not False):
        raise ValueError('decrypted checkpoint and receipt do not agree')
    verify_copies(plain, collection, common, budget)

    # Tools must be selected outside the archive. The decrypted tree is
    # retained as evidence with nonexecutable files and is never run.
    statectl_sha = policy['artifacts']['statectl']['sha256']
    copy_sha = policy['artifacts']['copy']['sha256']
    with (closing(common['Frozen'](args.statectl, 0, 512 * 1024**2, budget, secret=False)) as statectl,
          closing(common['Frozen'](args.key_verifier, 0, 512 * 1024**2, budget, secret=False)) as keys):
        if statectl.digest != statectl_sha or keys.digest != copy_sha:
            raise ValueError('independent verifier bytes differ from collected policy')
        report = common['COMBINED']['verify'](checkpoint, statectl.path, statectl_sha)
        inventory = document(read(checkpoint / 'cashu/inventory.json'))
        native = document(read(checkpoint / 'native/MANIFEST.json'))
        configured_keys = verify_key_bindings(plain, checkpoint, inventory, native, policy, common, budget)
        key_report = common['invoke'](keys.path, ['verify-recovery-keys', plain / 'recovery-material'], budget)
        statectl.check()
        keys.check()
    if (report['checkpoint_id'] != receipt['checkpoint_id']
            or report['restore_authorized'] is not False or report['resume_authorized'] is not False
            or key_report != collection['key_verification']
            or key_report['wallet_key_binding_verified'] is not True):
        raise ValueError('checkpoint or recovery key replay differs')
    plan = image_plan(checkpoint, inventory, native)
    plan['wallet'].extend((plain / 'recovery-material' / name, Path(name)) for name in configured_keys)
    images = destination / 'fenced-images'
    images.mkdir(mode=0o700)
    for role in plan:
        directory = images / role
        directory.mkdir(mode=0o700)
        fence = ('CASHU_RECOVERY_REQUIRED' if role in ('wallet', 'cashu_signer')
                 else 'CYBERHERD_RECOVERY_REQUIRED')
        common['write'](directory / fence, (receipt['checkpoint_id'] + '\n').encode())
        common['sync'](directory)
    common['sync'](images)
    image_records = {}
    for role, entries in plan.items():
        image_records[role] = []
        for source, name in entries:
            with closing(common['Frozen'](source, 0, 1024**3, budget)) as frozen:
                copied = frozen.copy(images / role / name)
            image_records[role].append({'name': str(name), **copied})
        if role in ('wallet', 'cashu_signer'):
            lock = 'wallet-writer.lock'
            common['write'](images / role / lock, b'')
            image_records[role].append({'name': lock, 'sha256': common['sha'](b''), 'bytes': 0})
        common['sync'](images / role)
    for directory in sorted((p for p in plain.rglob('*') if p.is_dir()), reverse=True):
        common['sync'](directory)
    common['sync'](plain)
    image_manifest = destination / 'FENCED_IMAGES.json'
    common['write_json'](image_manifest, {
        'version': 1, 'checkpoint_id': receipt['checkpoint_id'],
        'ciphertext_sha256': args.archive_sha256, 'roles': image_records,
    })
    result = {'result': 'CASHU_RESTORE_STAGED', 'version': 1,
              'checkpoint_id': receipt['checkpoint_id'], 'ciphertext_sha256': args.archive_sha256,
              'tool': tool_identity, 'fenced_roles': sorted(plan), 'keys_verified': True,
              'fenced_images_sha256': common['sha'](image_manifest.read_bytes()),
              'collection_sha256': common['sha'](collection_raw),
              'external_reconciliation_verified': False, 'restore_authorized': False,
              'resume_authorized': False}
    common['write_json'](destination / 'STAGED.json', result)
    common['sync'](destination)
    temporary_tar.unlink()
    marker.unlink()
    common['sync'](destination)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path)
    parser.add_argument('archive_sha256')
    parser.add_argument('receipt', type=Path)
    parser.add_argument('identity', type=Path)
    parser.add_argument('destination', type=Path)
    parser.add_argument('--age', required=True, type=Path)
    parser.add_argument('--age-sha256', required=True)
    parser.add_argument('--statectl', required=True, type=Path)
    parser.add_argument('--key-verifier', required=True, type=Path)
    parser.add_argument('--max-bytes', required=True, type=int)
    parser.add_argument('--deadline-seconds', type=int, default=300)
    args = parser.parse_args()
    try:
        tool = verified_tool_identity()
        common = runpy.run_path(str(HERE / 'collect-cashu-backup.py'))
        print(json.dumps(stage(args, common, tool), sort_keys=True))
    except (ValueError, OSError, KeyError, TypeError, StopIteration, sqlite3.Error,
            tarfile.TarError, subprocess.SubprocessError):
        parser.exit(1, 'Cashu restore staging refused. Preserve any incomplete private stage.\n')


if __name__ == '__main__':
    if 'verified_tool_identity' not in globals():
        raise SystemExit('use the externally verified root-owned backup launcher')
    main()
