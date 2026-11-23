#!/usr/bin/env python3
"""Exercise the actual pinned SQLite copy helper with disposable root-owned files."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import pwd
import resource
import shutil
import sqlite3
import subprocess
import tempfile

MARKER = b'LIGHTNING_GOATS_CASHU_BACKUP_COPY_V1\n'
ERROR = b'Database copy normalization refused; preserve the incomplete private stage.\n'

def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as source:
        for block in iter(lambda: source.read(65536), b''):
            value.update(block)
    return value.hexdigest()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--cli-sha256', required=True)
    args = parser.parse_args()
    if os.geteuid() != 0 or os.readlink('/proc/self/ns/net') == os.readlink('/proc/1/ns/net'):
        raise ValueError('requires root in a fresh network namespace')
    links = json.loads(subprocess.check_output(['/usr/sbin/ip', '-j', 'link', 'show']))
    if [link['ifname'] for link in links] != ['lo']:
        raise ValueError('fixture requires loopback-only namespace')
    source = args.cli.resolve(strict=True)
    if not 0 < source.stat().st_size <= 512 * 1024 * 1024 or digest(source) != args.cli_sha256:
        raise ValueError('explicit native artifact mismatch')
    used = {p.pw_uid for p in pwd.getpwall()}
    used |= {int(v) for v in subprocess.check_output(['ps','-e','-o','uid='],text=True).split()}
    uid = next(v for v in range(61000,65000) if v not in used)
    with tempfile.TemporaryDirectory(prefix='lg-cashu-copy-', dir='/run') as temporary:
        root = Path(temporary)
        root.chmod(0o711)
        cli = root / 'copy-cli'
        shutil.copyfile(source, cli)
        cli.chmod(0o555)
        assert digest(cli) == args.cli_sha256

        def stage(name):
            path = root / name
            path.mkdir(mode=0o700)
            (path/'INCOMPLETE').write_bytes(MARKER)
            (path/'INCOMPLETE').chmod(0o600)
            return path

        def database(path, wal=False, bad_fk=False):
            connection = sqlite3.connect(path)
            connection.execute('PRAGMA foreign_keys=OFF')
            connection.executescript('CREATE TABLE retained(note TEXT,signature BLOB);'
                'CREATE TABLE parents(id INTEGER PRIMARY KEY);'
                'CREATE TABLE children(parent INTEGER REFERENCES parents(id));'
                'INSERT INTO parents VALUES(1); INSERT INTO children VALUES(1);')
            connection.commit()
            if wal:
                assert connection.execute('PRAGMA journal_mode=WAL').fetchone()[0]=='wal'
                connection.execute('PRAGMA wal_autocheckpoint=0')
                connection.execute('PRAGMA wal_checkpoint(TRUNCATE)')
            connection.execute('INSERT INTO retained(rowid,note,signature) VALUES(7,?,?)',
                               ('synthetic-private-proof', bytes(range(64))))
            connection.execute('INSERT INTO retained(rowid,note,signature) VALUES(101,?,?)',
                               ('synthetic-signed-byte-history', b'\x00\xff\x00'))
            if bad_fk:
                connection.execute('INSERT INTO children VALUES(999)')
            connection.commit()
            return connection

        def copied(name, wal=False, bad_fk=False):
            selected = stage(name)
            live = root / (name+'-live.db')
            connection = database(live, wal, bad_fk)
            for suffix in ('','-wal','-shm'):
                original = Path(str(live)+suffix)
                if original.exists():
                    target = selected / ('raw-state.db'+suffix)
                    shutil.copyfile(original, target)
                    target.chmod(0o600)
            expected = connection.execute('SELECT rowid,note,signature FROM retained ORDER BY rowid').fetchall()
            return selected, connection, expected

        def command(selected, input_name='raw-state.db', output_name='normalized-state.db', maximum=8*1024*1024, prefix=(), limit=None, deadline=90):
            def bounded_size():
                resource.setrlimit(resource.RLIMIT_CORE, (0,0))
                resource.setrlimit(resource.RLIMIT_FSIZE, (4096,4096))
            return subprocess.run([*prefix,str(cli),'normalize-copy',str(selected),input_name,output_name,str(maximum)],
                stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE,
                env={'PATH':'/usr/bin:/bin'},timeout=deadline,preexec_fn=bounded_size if limit else None)

        positive, negative = 0, 0
        for wal in (False,True):
            selected, connection, expected = copied('positive-'+str(wal),wal)
            before={n:digest(selected/n) for n in ('raw-state.db','raw-state.db-wal') if (selected/n).exists()}
            result=command(selected)
            if result.returncode:
                raise ValueError('actual native copy fixture failed (wal='+str(wal)+'): '+result.stderr.decode(errors='replace')[-2000:])
            receipt=json.loads(result.stdout)
            assert receipt['engine']=='3.54.1' and receipt['input_scope']=='protected_root_owned_copy_stage'
            for flag in ('application_runtime_started','quiescence_verified','coordinated_backup_verified','restore_authorized'):
                assert receipt[flag] is False
            assert receipt['source_main_sha256']==before['raw-state.db']
            assert receipt['source_wal_sha256']==before.get('raw-state.db-wal')
            normalized=selected/'normalized-state.db'
            assert receipt['normalized_sha256']==digest(normalized)
            assert receipt['normalized_bytes']==normalized.stat().st_size
            assert normalized.stat().st_mode & 0o777==0o600 and normalized.stat().st_uid==0
            restored=sqlite3.connect('file:'+str(normalized)+'?mode=ro',uri=True)
            try:
                assert restored.execute('PRAGMA integrity_check').fetchone()==('ok',)
                assert restored.execute('PRAGMA foreign_key_check').fetchall()==[]
                assert restored.execute('SELECT rowid,note,signature FROM retained ORDER BY rowid').fetchall()==expected
            finally:
                restored.close()
            for name, value in before.items():
                assert digest(selected/name)==value
            assert (selected/'INCOMPLETE').read_bytes()==MARKER
            assert all(not (selected/('normalized-state.db'+suffix)).exists() for suffix in ('-wal','-shm','-journal'))
            if not wal:
                assert not (selected/'raw-state.db-wal').exists() and not (selected/'raw-state.db-shm').exists()
            assert b'synthetic-private-proof' not in result.stdout and b'synthetic-signed-byte-history' not in result.stdout
            repeated=command(selected)
            assert repeated.returncode!=0 and repeated.stdout==b'' and repeated.stderr==ERROR
            assert digest(normalized)==receipt['normalized_sha256']
            connection.close()
            positive+=1
            negative+=1

        for case in ('missing-marker','bad-marker','marker-link','marker-fifo','broad-directory','input-link','input-fifo','input-hardlink',
                     'wrong-owner','broad-file','wal-link','wal-fifo','rollback-journal','existing-output','orphan-output-wal',
                     'byte-bound','input-traversal','output-traversal','non-root'):
            selected, connection, _ = copied('negative-'+case)
            original=selected/'raw-state.db'
            before=digest(original)
            options={}
            if case=='missing-marker': (selected/'INCOMPLETE').unlink()
            elif case=='bad-marker': (selected/'INCOMPLETE').write_bytes(b'not-an-approved-copy-stage')
            elif case=='marker-link':
                (selected/'INCOMPLETE').unlink(); (selected/'INCOMPLETE').symlink_to('absent')
            elif case=='marker-fifo':
                (selected/'INCOMPLETE').unlink(); os.mkfifo(selected/'INCOMPLETE',0o600)
            elif case=='broad-directory': selected.chmod(0o755)
            elif case=='input-link':
                original.rename(selected/'retained.db'); original.symlink_to('retained.db')
            elif case=='input-fifo':
                original.rename(selected/'retained.db'); os.mkfifo(original,0o600)
            elif case=='input-hardlink': os.link(original,selected/'retained-hardlink.db')
            elif case=='wrong-owner': os.chown(original,uid,uid)
            elif case=='broad-file': original.chmod(0o644)
            elif case=='wal-link': (selected/'raw-state.db-wal').symlink_to('absent-wal')
            elif case=='wal-fifo': os.mkfifo(selected/'raw-state.db-wal',0o600)
            elif case=='rollback-journal': (selected/'raw-state.db-journal').touch(mode=0o600)
            elif case=='existing-output': (selected/'normalized-state.db').write_bytes(b'preserve-existing-output')
            elif case=='orphan-output-wal': (selected/'normalized-state.db-wal').touch(mode=0o600)
            elif case=='byte-bound': options['maximum']=4096
            elif case=='input-traversal': options['input_name']='../private-source-token'
            elif case=='output-traversal': options['output_name']='../private-output-token'
            elif case=='non-root':
                options['prefix']=('/usr/bin/setpriv',f'--reuid={uid}',f'--regid={uid}','--clear-groups','--no-new-privs','--')
            if case.endswith('-fifo'): options['deadline']=3
            result=command(selected,**options)
            assert result.returncode!=0 and result.stdout==b'' and result.stderr==ERROR, case
            if case not in ('existing-output',):
                assert not (selected/'normalized-state.db').exists(),case
            else:
                assert (selected/'normalized-state.db').read_bytes()==b'preserve-existing-output'
            retained=selected/'retained.db' if case in ('input-link','input-fifo') else original
            assert digest(retained)==before
            connection.close()
            negative+=1

        selected, connection, _=copied('foreign-key-refusal',bad_fk=True)
        result=command(selected)
        assert result.returncode!=0 and result.stdout==b'' and result.stderr==ERROR
        assert not (selected/'normalized-state.db').exists()
        connection.close(); negative+=1
        selected=stage('corrupt-refusal')
        (selected/'raw-state.db').write_bytes(b'synthetic-private-not-a-database')
        (selected/'raw-state.db').chmod(0o600)
        result=command(selected)
        assert result.returncode!=0 and result.stdout==b'' and result.stderr==ERROR
        assert not (selected/'normalized-state.db').exists()
        negative+=1

        # A WAL reader may grow shared memory before creating its destination;
        # keep this fault specifically at normalized-output creation/write.
        # Committed WAL inclusion and preservation are covered above.
        selected, connection, _=copied('interrupted-output')
        before={p.name:digest(p) for p in selected.iterdir() if p.name in ('raw-state.db','raw-state.db-wal','INCOMPLETE')}
        result=command(selected,limit=True)
        assert result.returncode!=0 and result.stdout==b''
        assert (selected/'normalized-state.db').exists()
        for name,value in before.items(): assert digest(selected/name)==value
        repeated=command(selected)
        assert repeated.returncode!=0 and repeated.stdout==b'' and repeated.stderr==ERROR
        connection.close(); negative+=1
        print(json.dumps({'result':'CASHU_COPY_NORMALIZATION_VERIFIED','cli_sha256':args.cli_sha256,
            'positive_cases':positive,'negative_cases':negative,'engine':'3.54.1',
            'implicit_rowids_and_signed_bytes_preserved':True,'committed_wal_included':True,
            'interrupted_stage_retained':True,'quiescence_or_restore_qualified':False,
            'live_financial_or_physical_operations':False},sort_keys=True))

if __name__=='__main__':
    main()
