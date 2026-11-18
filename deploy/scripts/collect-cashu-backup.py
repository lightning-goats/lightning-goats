#!/usr/bin/env python3
"""Collect all stopped native/Cashu writers into a verified encrypted checkpoint.

Authenticated launcher only. Never stops/starts a service, opens original SQLite
through SQLite, restores state, clears a recovery fence or contacts a provider.
"""
import sys
sys.dont_write_bytecode = True
if 'verified_tool_identity' not in globals():
    if __name__ == '__main__':
        raise SystemExit('use the externally verified root-owned backup launcher')
    def verified_tool_identity():
        raise ValueError('verified backup launcher required')

import argparse
from contextlib import ExitStack, closing
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import resource
import runpy
import sqlite3
import stat
import subprocess
import time
import tomllib
import uuid

HERE = Path(__file__).resolve().parent
COMBINED = runpy.run_path(str(HERE / 'verify-cashu-combined-backup.py'))
MARKER = b'LIGHTNING_GOATS_CASHU_BACKUP_COPY_V1\n'
GLOBAL_LOCK = Path('/var/lib/lightning-goats-upgrades/CYBERHERD-OVERLAY-MAINTENANCE.lock')
ROLES = {'core', 'membership', 'payout', 'signer', 'wallet', 'cashu_signer'}
TOOLS = {'copy', 'statectl', 'age', 'tar'}


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def document(raw):
    return json.loads(raw, object_pairs_hook=COMBINED['CASHU']['unique_fields'])


def canonical(path):
    path = Path(path)
    if not path.is_absolute() or path != path.resolve(strict=True):
        raise ValueError('canonical absolute source required')
    return path


def protected_directory(path, uid=0, gid=None, private=False):
    path = canonical(path)
    for index, ancestor in enumerate([path, *path.parents]):
        info = ancestor.lstat()
        if (not stat.S_ISDIR(info.st_mode) or info.st_uid != (uid if index == 0 else 0)
                or info.st_mode & (0o077 if private and index == 0 else 0o022)
                or (index == 0 and gid is not None and info.st_gid != gid)):
            raise ValueError('protected source directory required')
    return path


def identity(info):
    return (info.st_dev, info.st_ino, info.st_uid, info.st_gid, info.st_mode,
            info.st_nlink, info.st_size, info.st_mtime_ns, info.st_ctime_ns)


class Budget:
    def __init__(self, limit, seconds):
        self.limit, self.bytes = limit, 0
        self.deadline = time.monotonic() + seconds
        self.source_owners=set()

    def remaining(self, ceiling=120):
        value = min(ceiling, self.deadline - time.monotonic())
        if value <= 0:
            raise ValueError('collection deadline exceeded')
        return value

    def add(self, count):
        self.remaining()
        self.bytes += count
        if self.bytes > self.limit:
            raise ValueError('aggregate collection size exceeded')


class Frozen:
    def __init__(self, path, uid, maximum, budget, *, secret=True, gid=None, empty=False):
        self.path = canonical(path)
        for parent in self.path.parents:
            info=parent.lstat()
            if (not stat.S_ISDIR(info.st_mode)
                    or (info.st_uid == 0 and info.st_mode & 0o022)
                    or (info.st_uid != 0 and (info.st_uid not in {uid,*budget.source_owners} or info.st_mode & 0o077))):
                raise ValueError('unprotected source ancestry')
        self.file = os.fdopen(os.open(self.path, os.O_RDONLY | os.O_NOFOLLOW |
                                      os.O_NONBLOCK | os.O_CLOEXEC), 'rb')
        try:
            info = os.fstat(self.file.fileno())
            if (not stat.S_ISREG(info.st_mode) or info.st_uid != uid or info.st_nlink != 1
                    or info.st_mode & (0o077 if secret else 0o022)
                    or (gid is not None and info.st_gid != gid)
                    or not int(not empty) <= info.st_size <= maximum):
                raise ValueError('bounded protected regular source required')
            self.expected, self.budget = identity(info), budget
            self.digest = self.hash()
            self.check()
        except BaseException:
            self.file.close()
            raise

    def close(self):
        self.file.close()

    def hash(self):
        self.file.seek(0)
        digest, size = hashlib.sha256(), 0
        while chunk := self.file.read(65536):
            self.budget.remaining()
            size += len(chunk)
            if size > self.expected[6]:
                raise ValueError('source grew during fingerprint')
            digest.update(chunk)
        if size != self.expected[6]:
            raise ValueError('source changed size')
        return digest.hexdigest()

    def check(self):
        if (identity(os.fstat(self.file.fileno())) != self.expected
                or identity(self.path.lstat()) != self.expected
                or self.hash() != self.digest
                or identity(os.fstat(self.file.fileno())) != self.expected
                or identity(self.path.lstat()) != self.expected):
            raise ValueError('original source changed during collection')

    def copy(self, target):
        self.check()
        self.file.seek(0)
        with target.open('xb') as out:
            os.fchmod(out.fileno(), 0o600)
            count=0
            while chunk := self.file.read(65536):
                count+=len(chunk)
                if count>self.expected[6]:
                    raise ValueError('source grew during copy')
                self.budget.add(len(chunk))
                out.write(chunk)
            out.flush()
            os.fsync(out.fileno())
        self.check()
        return {'sha256': self.digest, 'bytes': self.expected[6]}


def write(path, raw):
    with path.open('xb') as output:
        os.fchmod(output.fileno(), 0o600)
        output.write(raw)
        output.flush()
        os.fsync(output.fileno())


def json_bytes(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode() + b'\n'


def write_json(path, value):
    write(path, json_bytes(value))


def sync(path):
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def absent(path):
    if os.path.lexists(path):
        raise ValueError('preexisting or uncertain source/output refused')


def check_quiescence(workers, budget):
    names = 'Id,LoadState,ActiveState,SubState,MainPID,ControlPID,UnitFileState,ControlGroup'
    for worker in workers.values():
        result = subprocess.run(['/usr/bin/systemctl', 'show', '--no-pager',
            '--property=' + names, '--', worker['unit']], stdin=subprocess.DEVNULL,
            capture_output=True, timeout=budget.remaining(5), env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'})
        if result.returncode or len(result.stdout) > 16384:
            raise ValueError('stopped writer status unavailable')
        pairs = [line.split('=', 1) for line in result.stdout.decode().splitlines()]
        values = dict(pairs)
        if (len(pairs) != 8 or set(values) != set(names.split(','))
                or values['Id'] != worker['unit'] or values['MainPID'] != '0'
                or values['ControlPID'] != '0' or values['ActiveState'] != 'inactive'
                or values['SubState'] != 'dead' or values['LoadState'] not in ('loaded','masked')
                or values['UnitFileState'] not in ('masked','masked-runtime')):
            raise ValueError('selected writers must remain masked and inactive')
        group = values['ControlGroup']
        if group:
            if not group.startswith('/') or any(v in ('.','..') for v in group.split('/')):
                raise ValueError('invalid writer control group')
            events = Path('/sys/fs/cgroup') / group.lstrip('/') / 'cgroup.events'
            try:
                if 'populated 0' not in events.read_text().splitlines():
                    raise ValueError('writer control group remains populated')
            except FileNotFoundError:
                pass
    uids = {w['uid'] for w in workers.values()}
    for entry in Path('/proc').iterdir():
        if not entry.name.isdecimal():
            continue
        try:
            text = (entry / 'status').read_text()
        except (FileNotFoundError, ProcessLookupError):
            continue
        for line in text.splitlines():
            if line.startswith('Uid:') and uids.intersection(map(int, line.split()[1:])):
                raise ValueError('a runtime UID still has a process')


def receiving_material(wallet):
    active = wallet['receiving_key']
    anchor = wallet.get('receiving_key_anchor')
    if anchor is None:
        anchor = active
    retained = wallet.get('retained_receiving_keys', [])
    if not isinstance(retained, list) or len(retained) > 16:
        raise ValueError('invalid configured receiving key inventory')
    keys = [active, anchor, *(row['public_key'] for row in retained)]
    if any(not re.fullmatch(r'0[23][0-9a-f]{64}', key) for key in keys):
        raise ValueError('invalid configured receiving key')
    if len(set(keys)) != len({key[2:] for key in keys}):
        raise ValueError('conflicting configured receiving key parity')
    return anchor, {f'receiving-key-{key}.bin': key for key in sorted(set(keys) - {anchor})}


def load_policy(path, expected, budget):
    protected_directory(path.parent, private=True)
    with closing(Frozen(path, 0, 65536, budget)) as frozen:
        if frozen.digest != expected:
            raise ValueError('independently reviewed policy digest required')
        frozen.file.seek(0)
        raw = frozen.file.read()
    p = document(raw)
    fields = {'version', 'operator_exclusive', 'workers', 'native_policy', 'wallet_config',
              'core_config', 'membership_config', 'payout_config',
              'cashu_signer_config', 'payout_signer_config', 'cashu_client_source',
              'payout_client_source', 'historical_receiving_keys', 'extra_files',
              'artifacts', 'runtime_artifacts', 'recipient', 'component_bytes', 'total_bytes', 'deadline_seconds'}
    if (set(p) != fields or type(p['version']) is not int or p['version'] != 1 or p['operator_exclusive'] is not True
            or set(p['workers']) != ROLES or set(p['artifacts']) != TOOLS or set(p['runtime_artifacts']) != ROLES
            or not re.fullmatch('age1[023456789acdefghjklmnpqrstuvwxyz]{58}', p['recipient'])
            or type(p['component_bytes']) is not int or not 4096 <= p['component_bytes'] <= 1024**3
            or type(p['total_bytes']) is not int or not p['component_bytes'] <= p['total_bytes'] <= 64*1024**3
            or type(p['deadline_seconds']) is not int or not 30 <= p['deadline_seconds'] <= 1800
            or not isinstance(p['historical_receiving_keys'], list) or len(p['historical_receiving_keys']) > 64
            or not isinstance(p['extra_files'], list) or len(p['extra_files']) > 128):
        raise ValueError('invalid collection policy scope')
    seen_uids, seen_units, seen_dirs = set(), set(), set()
    for w in p['workers'].values():
        if (set(w) != {'unit','uid','gid','state'} or type(w['uid']) is not int or w['uid'] <= 0
                or type(w['gid']) is not int or w['gid'] <= 0
                or not re.fullmatch(r'[A-Za-z0-9_.@-]{1,128}\.service', w['unit'])
                or w['uid'] in seen_uids or w['unit'] in seen_units):
            raise ValueError('distinct non-root writer identities required')
        state = protected_directory(Path(w['state']), w['uid'], w['gid'], private=True)
        if any(state == other or state in other.parents or other in state.parents for other in seen_dirs):
            raise ValueError('writer directories overlap')
        seen_uids.add(w['uid']); seen_units.add(w['unit']); seen_dirs.add(state)
    for value in [*p['artifacts'].values(), *p['runtime_artifacts'].values()]:
        if set(value) != {'path','sha256'} or not re.fullmatch('[0-9a-f]{64}', value['sha256']):
            raise ValueError('explicit selected artifact hashes required')
    return p, raw


def invoke(program, arguments, budget):
    result = subprocess.run([str(program), *map(str, arguments)], stdin=subprocess.DEVNULL,
        capture_output=True, timeout=budget.remaining(120), env={'PATH':'/usr/bin:/bin','LC_ALL':'C'})
    if result.returncode or len(result.stdout) > 65536 or len(result.stderr) > 65536:
        raise ValueError('selected native collector operation refused')
    return document(result.stdout)


def encrypt(plain, destination, programs, recipient, budget):
    # Fixed archive members only. Never include our in-progress ciphertext.
    members = sorted(p.name for p in plain.iterdir())
    budget.bytes=sum(p.stat().st_size for p in plain.rglob('*') if p.is_file())
    maximum_ciphertext=budget.limit-budget.bytes
    if maximum_ciphertext<=0: raise ValueError('no aggregate budget remains for ciphertext')
    def bounded_ciphertext():
        resource.setrlimit(resource.RLIMIT_CORE,(0,0))
        resource.setrlimit(resource.RLIMIT_FSIZE,(maximum_ciphertext,maximum_ciphertext))
    with destination.open('xb') as output:
        os.fchmod(output.fileno(), 0o600)
        producer = subprocess.Popen([str(programs['tar']), '--format=ustar', '--sort=name',
            '--numeric-owner','--owner=0','--group=0','--mtime=@0','-C',str(plain),'-cf','-',
            '--',*members], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, env={'PATH':'/usr/bin:/bin','LC_ALL':'C'})
        consumer = None
        try:
            consumer = subprocess.Popen([str(programs['age']), '--encrypt', '--recipient', recipient],
                stdin=producer.stdout, stdout=output, stderr=subprocess.DEVNULL,
                env={'PATH':'/usr/bin:/bin','LC_ALL':'C'},preexec_fn=bounded_ciphertext)
            producer.stdout.close()
            while producer.poll() is None or consumer.poll() is None:
                budget.remaining()
                if output.tell() > maximum_ciphertext:
                    raise ValueError('ciphertext size exceeds collection bound')
                if consumer.poll() not in (None,0) or producer.poll() not in (None,0):
                    raise ValueError('encrypted archive pipeline refused')
                time.sleep(0.025)
            if producer.returncode or consumer.returncode or not 0 < output.tell() <= maximum_ciphertext:
                raise ValueError('encrypted archive did not complete')
            budget.add(output.tell())
            output.flush(); os.fsync(output.fileno())
        finally:
            for child in (producer, consumer):
                if child is not None:
                    if child.poll() is None:
                        child.kill()
                    child.wait(timeout=5)
            if producer.stdout is not None:
                producer.stdout.close()


def collect(policy_path, expected, destination):
    if os.geteuid() != 0:
        raise ValueError('root collector required')
    tool = verified_tool_identity()
    # The bootstrap bound applies before a policy can choose its tighter limits.
    bootstrap = Budget(1024*1024, 30)
    p, policy_raw = load_policy(canonical(policy_path), expected, bootstrap)
    budget = Budget(p['total_bytes'], p['deadline_seconds'])
    budget.source_owners={w['uid'] for w in p['workers'].values()}
    destination = Path(destination)
    protected_directory(destination.parent, private=True)
    if (not destination.is_absolute() or destination.name in ('.','..')
            or any(destination == Path(w['state']) or Path(w['state']) in destination.parents
                   or destination in Path(w['state']).parents for w in p['workers'].values())):
        raise ValueError('new private backup destination required')
    protected_directory(GLOBAL_LOCK.parent, private=True)
    sources = []
    with ExitStack() as stack:
        policy_lease=stack.enter_context(closing(Frozen(canonical(policy_path),0,65536,budget)))
        if policy_lease.digest!=expected:raise ValueError('reviewed policy changed before collection')
        sources.append(policy_lease)
        global_fd = os.open(GLOBAL_LOCK, os.O_RDWR|os.O_CREAT|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC,0o600)
        global_stream = stack.enter_context(os.fdopen(global_fd,'rb'))
        info = os.fstat(global_fd)
        if not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o077 or info.st_nlink != 1:
            raise ValueError('unsafe global maintenance lock')
        fcntl.flock(global_fd, fcntl.LOCK_EX|fcntl.LOCK_NB)
        global_identity=identity(os.fstat(global_fd))
        check_quiescence(p['workers'], budget)
        for role in ('wallet','cashu_signer'):
            w = p['workers'][role]
            frozen = stack.enter_context(closing(Frozen(Path(w['state'])/'wallet-writer.lock',
                w['uid'],4096,budget,gid=w['gid'],empty=True)))
            fcntl.flock(frozen.file,fcntl.LOCK_EX|fcntl.LOCK_NB)
            sources.append(frozen)
        destination.mkdir(mode=0o700)
        write(destination/'INCOMPLETE', b'LIGHTNING_GOATS_COORDINATED_CASHU_COPY_V1\n')
        plain = destination/'plaintext'; plain.mkdir(mode=0o700)
        raw = plain/'raw'; raw.mkdir(mode=0o700); write(raw/'INCOMPLETE',MARKER)
        keys = plain/'recovery-material'; keys.mkdir(mode=0o700); write(keys/'INCOMPLETE',MARKER)
        programs_dir = plain/'programs'; programs_dir.mkdir(mode=0o700)
        checkpoint = plain/'checkpoint'; checkpoint.mkdir(mode=0o700)
        cashu = checkpoint/'cashu'; cashu.mkdir(mode=0o700)
        native = checkpoint/'native'; native.mkdir(mode=0o700)
        write(plain/'collection-policy.json',policy_raw)
        copied, expected_files, normalizations = [], {r:set() for r in ROLES}, []

        def copy_source(original, target, *, uid=0, gid=None, secret=True, maximum=None, empty=False):
            original = canonical(original)
            frozen = stack.enter_context(closing(Frozen(original,uid,maximum or p['component_bytes'],
                budget,secret=secret,gid=gid,empty=empty)))
            sources.append(frozen)
            record = frozen.copy(target)
            copied.append({'source':str(original),'path':str(target.relative_to(plain)),
                'source_uid':frozen.expected[2],'source_gid':frozen.expected[3],
                'source_mode':stat.S_IMODE(frozen.expected[4]),**record})
            for role,w in p['workers'].items():
                if original.is_relative_to(Path(w['state'])):
                    expected_files[role].add(str(original.relative_to(Path(w['state']))))
            return record

        programs = {}
        for name, selected in p['artifacts'].items():
            target = programs_dir/name
            rec = copy_source(selected['path'],target,secret=False,maximum=512*1024*1024)
            if rec['sha256'] != selected['sha256']:
                raise ValueError('selected collector artifact hash differs')
            target.chmod(0o500); programs[name]=target
        for role,selected in p['runtime_artifacts'].items():
            target=programs_dir/('runtime-'+role)
            rec=copy_source(selected['path'],target,secret=False,maximum=512*1024*1024)
            if rec['sha256']!=selected['sha256']:
                raise ValueError('selected runtime artifact hash differs')
            target.chmod(0o500)
        version = subprocess.run([str(programs['age']),'--version'],capture_output=True,
            stdin=subprocess.DEVNULL,timeout=budget.remaining(5),env={'PATH':'/usr/bin:/bin'})
        if version.returncode or version.stdout.strip() != b'v1.3.2':
            raise ValueError('qualified age version required')
        configs = {}
        for role,field in [('native','native_policy'),('core','core_config'),
                           ('membership','membership_config'),('payout','payout_config'),('wallet','wallet_config'),
                           ('cashu_signer','cashu_signer_config'),('signer','payout_signer_config')]:
            target = plain/(role+'-config')
            copy_source(p[field],target,secret=False,maximum=65536)
            data=target.read_bytes()
            configs[role]=document(data) if role in ('wallet','cashu_signer') else tomllib.loads(data.decode())
        policy=configs['native']; wallet=configs['wallet']; signer=configs['cashu_signer']
        if set(policy['components']) != {'core','membership','payout','signer'}:
            raise ValueError('complete native maintenance policy required')
        for role,c in policy['components'].items():
            w=p['workers'][role]
            if (c['uid'] != w['uid'] or c['gid'] != w['gid'] or c['unit'] != w['unit']
                    or Path(c['database']).parent != Path(w['state'])):
                raise ValueError('native writer binding differs')
        if (wallet['runtime_uid'] != p['workers']['wallet']['uid']
                or wallet['state_directory'] != p['workers']['wallet']['state']
                or signer['runtime_uid'] != p['workers']['cashu_signer']['uid']
                or signer['state_directory'] != p['workers']['cashu_signer']['state']
                or configs['signer']['state_dir'] != p['workers']['signer']['state']
                or configs['membership']['state_dir'] != p['workers']['membership']['state']
                or configs['payout']['state_dir'] != p['workers']['payout']['state']
                or configs['core']['database']['url'] != 'sqlite://'+policy['components']['core']['database']
                or wallet['core_uid'] != p['workers']['core']['uid']):
            raise ValueError('wallet or signer state binding differs')
        owner=policy['owner']
        if (signer['policy']['owner'] != owner or configs['signer']['policy']['owner'] != owner
                or signer['policy']['wallet_uid'] != wallet['runtime_uid']
                or signer['policy']['wallet_instance'] != wallet['wallet_instance']
                or signer['policy']['wallet_epoch'] != wallet['wallet_epoch']
                or len(wallet['receiving_key'])!=66 or wallet['receiving_key'][:2] not in ('02','03')
                or signer['policy']['receiving_key'] != wallet['receiving_key'][2:]):
            # Wallet configuration retains the compressed key; the signing
            # policy retains its Nostr x-only encoding. Native key verification
            # below derives/validates the point through the pinned library.
            raise ValueError('signer owner, wallet registration or receiving binding differs')
        components=[]

        def component(role, source, **extra):
            target=cashu/(str(len(components))+'.component')
            rec=copy_source(source,target,**extra.pop('permissions',{}))
            components.append({'role':role,'path':target.name,'sha256':rec['sha256'],**extra})
            return target

        def database(original, role):
            w=p['workers'][role]; original=Path(original)
            index=len(normalizations); name=f'raw-{index}.db'; output=f'normalized-{index}.db'
            copy_source(original,raw/name,uid=w['uid'],gid=w['gid'])
            absent(Path(str(original)+'-journal'))
            if os.path.lexists(Path(str(original)+'-shm')) and not os.path.lexists(Path(str(original)+'-wal')):
                raise ValueError('orphan original shared memory requires reconciliation')
            for suffix in ('-wal','-shm'):
                sidecar=Path(str(original)+suffix)
                if os.path.lexists(sidecar):
                    # Preserve original SHM bytes as evidence. SQLite rebuilds
                    # its working cache under the ordinary -shm name; allowing
                    # that cache to replace the retained copy falsifies its hash.
                    retained_name=name+suffix+('.original' if suffix=='-shm' else '')
                    copy_source(sidecar,raw/retained_name,uid=w['uid'],gid=w['gid'],empty=True)
                else:
                    # Remember absence too; appearing WAL data invalidates the set.
                    absent_paths.add(sidecar)
            rec=invoke(programs['copy'],['normalize-copy',raw,name,output,p['component_bytes']],budget)
            if (rec['version'] != 1 or rec['engine'] != '3.54.1'
                    or any(rec[k] is not False for k in ('quiescence_verified','coordinated_backup_verified','restore_authorized','application_runtime_started'))):
                raise ValueError('copy helper scope differs')
            normalized=raw/output
            budget.add(normalized.stat().st_size)
            normalizations.append(rec)
            return normalized

        absent_paths=set()
        images=[]
        for role,c in policy['components'].items():
            normalized=database(c['database'],role)
            if role=='core': component('core_ledger',normalized)
            target=native/(role+'.db'); rec=copy_source(normalized,target)
            images.append({'name':target.name,**rec})
        wallet_state=Path(p['workers']['wallet']['state'])
        journal=component('wallet_journal',database(wallet_state/'acquisition.sqlite','wallet'))
        component('cdk_wallet',database(wallet_state/'wallet.sqlite','wallet'))
        component('signing_broker',database(Path(p['workers']['cashu_signer']['state'])/'cashu-signing.sqlite','cashu_signer'))
        with closing(sqlite3.connect(journal.as_uri()+'?mode=ro&immutable=1',uri=True)) as db:
            db.execute('PRAGMA trusted_schema=OFF'); db.execute('PRAGMA query_only=ON')
            db.set_progress_handler(lambda:int(time.monotonic()>=budget.deadline),10000)
            namespaces=db.execute('SELECT mint_id,mint_url,ready FROM cdk_mint_namespaces LIMIT 257').fetchall()
            registration=db.execute('SELECT instance,key_binding FROM runtime_registration WHERE singleton=1').fetchall()
            epochs=db.execute('SELECT epoch FROM wallet_epoch WHERE singleton=1').fetchall()
        if len(namespaces)>230: raise ValueError('mint namespace checkpoint inventory exceeds bound')
        if len(registration)!=1 or len(epochs)!=1 or registration[0][0]!=wallet['wallet_instance'] or epochs[0][0]!=wallet['wallet_epoch']:
            raise ValueError('wallet registration differs from configuration')
        for mint,url,ready in namespaces:
            if ready!=1: raise ValueError('incomplete namespace requires recovery')
            name='mint-'+sha(b'lightning-goats/cdk-mint-store/v1\0'+mint.encode())+'.sqlite'
            component('cdk_mint_wallet',database(wallet_state/name,'wallet'),mint_id=mint,mint_url=url)
        key_permissions={'uid':p['workers']['wallet']['uid'],'gid':p['workers']['wallet']['gid']}
        for name,role in [('wallet-seed.bin','wallet_seed'),('receiving-key.bin','receiving_key')]:
            copied_key=component(role,wallet_state/name,permissions=key_permissions)
            copy_source(copied_key,keys/name)
        for role,field,name in [('cashu_signer','cashu_client_source','cashu-client.hex'),('signer','payout_client_source','payout-client.hex')]:
            copy_source(p[field],keys/name,maximum=65)
        historical=[]
        for index,h in enumerate(p['historical_receiving_keys']):
            if set(h) != {'source','public_key'}: raise ValueError('historical key binding required')
            name=f'historical-{index}.bin'
            copy_source(h['source'],keys/name,maximum=32)
            historical.append({'name':name,'public_key':h['public_key']})
        anchor, configured_keys = receiving_material(wallet)
        for name, public_key in configured_keys.items():
            copy_source(wallet_state / name, keys / name, **key_permissions, maximum=32)
            historical.append({'name': name, 'public_key': public_key})
        key_bindings={'version':1,'owner':owner,'receiving_key':anchor,
            'wallet_key_binding':registration[0][1].hex(), 'cashu_client':signer['nak']['client_pubkey'],
            'payout_client':configs['signer']['policy']['client_pubkey'],'historical':historical}
        write_json(keys/'key-bindings.json',key_bindings)
        key_result=invoke(programs['copy'],['verify-recovery-keys',keys],budget)
        if (key_result['version']!=1 or key_result['wallet_key_binding_verified'] is not True
                or key_result['signer_clients_verified']!=2 or key_result['historical_keys_verified']!=len(historical)
                or key_result['restore_authorized'] is not False or key_result['external_authentication_verified'] is not False):
            raise ValueError('recovery key scope differs')
        for field in ('core_config','membership_config','payout_config','wallet_config',
                      'cashu_signer_config','payout_signer_config','native_policy'):
            component('protected_policy',p[field],permissions={'secret':False})
        extras=plain/'additional'; extras.mkdir(mode=0o700)
        for index,item in enumerate(p['extra_files']):
            if set(item) != {'source','uid','secret'} or type(item['uid']) is not int or item['uid'] not in {0,*[w['uid'] for w in p['workers'].values()]} or type(item['secret']) is not bool:
                raise ValueError('invalid explicit additional recovery file')
            copy_source(item['source'],extras/str(index),uid=item['uid'],secret=item['secret'])
        # Every current state entry must be known, including retained recovery
        # markers. Nested helper state must be explicitly inventoried, not hidden.
        directories=[]; directory_fingerprints={}
        for role,w in p['workers'].items():
            if role in ('wallet','cashu_signer'): expected_files[role].add('wallet-writer.lock')
            for marker in ('CASHU_RECOVERY_REQUIRED','CYBERHERD_RECOVERY_REQUIRED'):
                source=Path(w['state'])/marker
                if os.path.lexists(source):
                    marker_uid=source.lstat().st_uid
                    if marker_uid not in (0,w['uid']): raise ValueError('unexpected recovery marker owner')
                    copy_source(source,extras/(role+'-'+marker),uid=marker_uid)
            actual=set()
            for folder,dirs,files in os.walk(w['state'],followlinks=False,onerror=lambda e: (_ for _ in ()).throw(e)):
                info=Path(folder).lstat()
                if not stat.S_ISDIR(info.st_mode) or info.st_uid!=w['uid'] or info.st_gid!=w['gid'] or info.st_mode&0o077:
                    raise ValueError('unprotected runtime subtree')
                directories.append({'role':role,'path':str(Path(folder).relative_to(w['state'])),
                                    'uid':info.st_uid,'gid':info.st_gid,'mode':stat.S_IMODE(info.st_mode)})
                directory_fingerprints[Path(folder)]=identity(info)
                if len(directories)>256: raise ValueError('runtime directory inventory exceeds bound')
                for name in dirs+files:
                    entry=Path(folder)/name; meta=entry.lstat()
                    if not(stat.S_ISREG(meta.st_mode) or stat.S_ISDIR(meta.st_mode)):
                        raise ValueError('special runtime state entry refused')
                    if stat.S_ISREG(meta.st_mode): actual.add(str(entry.relative_to(w['state'])))
            if actual != expected_files[role]:
                raise ValueError('unlisted runtime state entry requires explicit recovery inventory')
        helpers=[]
        for role,value in [('cashu_signer',signer['nak']['private_directory']),
                           ('signer',configs['signer']['nostr']['nak_config_path'])]:
            helper=Path(value); w=p['workers'][role]
            if not helper.is_absolute(): raise ValueError('absolute signer helper state required')
            if not os.path.lexists(helper):
                absent_paths.add(helper); helpers.append({'role':role,'path':str(helper),'absent':True})
                continue
            helpers.append({'role':role,'path':str(helper),'absent':False})
            if helper.is_relative_to(Path(w['state'])): continue
            protected_directory(helper,w['uid'],w['gid'],private=True)
            expected_helper={Path(item['source']) for item in copied if Path(item['source']).is_relative_to(helper)}
            actual_helper=set()
            for folder,dirs,files in os.walk(helper,followlinks=False,onerror=lambda e: (_ for _ in ()).throw(e)):
                meta=Path(folder).lstat()
                if not stat.S_ISDIR(meta.st_mode) or meta.st_uid!=w['uid'] or meta.st_gid!=w['gid'] or meta.st_mode&0o077:
                    raise ValueError('unprotected signer helper subtree')
                directory_fingerprints[Path(folder)]=identity(meta)
                directories.append({'role':role+'_helper','path':str(Path(folder).relative_to(helper)),
                                    'uid':meta.st_uid,'gid':meta.st_gid,'mode':stat.S_IMODE(meta.st_mode)})
                if len(directories)>256:raise ValueError('helper directory inventory exceeds bound')
                for name in dirs+files:
                    path=Path(folder)/name; entry=path.lstat()
                    if not(stat.S_ISDIR(entry.st_mode) or stat.S_ISREG(entry.st_mode)):
                        raise ValueError('special signer helper entry refused')
                    if stat.S_ISREG(entry.st_mode):actual_helper.add(path)
            if actual_helper!=expected_helper:raise ValueError('unlisted signer helper recovery material')
        registrations={'core_epoch':policy['epoch'],'wallet_instance':wallet['wallet_instance'],'wallet_epoch':wallet['wallet_epoch']}
        checkpoint_id=str(uuid.uuid4())
        write_json(cashu/'inventory.json',{'version':1,'checkpoint_id':checkpoint_id,'registrations':registrations,'components':components})
        maintenance=checkpoint/'maintenance-policy.toml'; copy_source(p['native_policy'],maintenance,secret=False)
        manifest={'version':2,'purpose':'cashu_four_journal_correlation_review','id':checkpoint_id,
            'policy':policy,'source_policy_sha256':sha(maintenance.read_bytes()),
            'cashu_inventory_sha256':sha((cashu/'inventory.json').read_bytes()),'images':images}
        write_json(native/'MANIFEST.json',manifest)
        write_json(checkpoint/'combined.json',{'version':1,'checkpoint_id':checkpoint_id,
            'cashu_inventory_sha256':manifest['cashu_inventory_sha256'],
            'native_manifest_sha256':sha((native/'MANIFEST.json').read_bytes()),
            'maintenance_policy_sha256':manifest['source_policy_sha256']})
        review=COMBINED['verify'](checkpoint,programs['statectl'],p['artifacts']['statectl']['sha256'])
        write_json(plain/'normalization.json',normalizations)
        write_json(plain/'collection.json',{'version':1,'checkpoint_id':checkpoint_id,'tool':tool,
            'policy_sha256':expected,'originals':copied,'directories':directories,
            'signer_helper_directories':helpers,'key_verification':key_result,
            'combined_review':review,'quiescence_verified':True,'external_reconciliation_verified':False,
            'restore_authorized':False,'resume_authorized':False})
        check_quiescence(p['workers'],budget)
        for source in sources: source.check()
        for path,expected_directory in directory_fingerprints.items():
            if identity(path.lstat())!=expected_directory: raise ValueError('runtime directory changed')
        for path in absent_paths: absent(path)
        if identity(GLOBAL_LOCK.lstat())!=global_identity:raise ValueError('maintenance lock identity changed')
        archive=destination/'checkpoint.tar.age'
        encrypt(plain,archive,programs,p['recipient'],budget)
        check_quiescence(p['workers'],budget)
        for source in sources: source.check()
        for path,expected_directory in directory_fingerprints.items():
            if identity(path.lstat())!=expected_directory: raise ValueError('runtime directory changed')
        for path in absent_paths: absent(path)
        if identity(GLOBAL_LOCK.lstat())!=global_identity:raise ValueError('maintenance lock identity changed')
        archive_copy=stack.enter_context(closing(Frozen(archive,0,p['total_bytes'],budget)))
        result={'result':'CASHU_COORDINATED_BACKUP_COLLECTED','version':1,'checkpoint_id':checkpoint_id,
            'policy_sha256':expected,'ciphertext_sha256':archive_copy.digest,'ciphertext_bytes':archive_copy.expected[6],
            'quiescence_verified':True,'current_key_bindings_verified':True,'historical_keys_copied':len(historical),
            'recovery_key_possession_verified':False,'plaintext_retained':True,
            'external_reconciliation_verified':False,'restore_authorized':False,'resume_authorized':False,
            'services_changed':False,'original_state_changed':False}
        write_json(destination/'COMPLETE.json',result)
        sync(plain); sync(destination)
        (destination/'INCOMPLETE').unlink(); sync(destination); sync(destination.parent)
        return result


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('policy',type=Path); parser.add_argument('policy_sha256')
    parser.add_argument('destination',type=Path)
    args=parser.parse_args()
    try:
        print(json.dumps(collect(args.policy,args.policy_sha256,args.destination),sort_keys=True))
    except (ValueError,OSError,KeyError,TypeError,sqlite3.Error,subprocess.SubprocessError):
        parser.exit(1,'Cashu collection refused; preserve the incomplete private checkpoint.\n')


if __name__=='__main__':
    main()
