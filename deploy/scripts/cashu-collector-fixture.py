"""Actual native/key/encryption composition; service statuses are explicitly mocked."""
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import runpy
import shutil
import sqlite3
import subprocess
import tarfile
import time
import types
from unittest.mock import patch


def digest(path):
    h=hashlib.sha256()
    with path.open('rb') as source:
        for chunk in iter(lambda:source.read(65536),b''): h.update(chunk)
    return h.hexdigest()


def qualify_restore(source, output_root, collected, identity, selected, store):
    """Pass the real encrypted collector output through the isolated launcher."""
    scripts = source / 'deploy/scripts'
    capsule = output_root / 'restore-capsule'
    capsule.mkdir(mode=0o700)
    files = runpy.run_path(str(scripts / 'launch-cashu-backup.py'))['FILES']
    for name in files:
        target = capsule / name
        shutil.copyfile(scripts / name, target)
        target.chmod(0o400)
    manifest = capsule / 'TOOL.json'
    manifest.write_text(json.dumps({'version': 1, 'source_commit': '01' * 20,
                                    'files': {name: digest(capsule / name) for name in files}}))
    manifest.chmod(0o600)
    ciphertext = collected / 'checkpoint.tar.age'
    checksum = digest(ciphertext)
    receipt = collected / 'COMPLETE.json'

    def invoke(destination, digest_override=checksum, key=identity):
        return subprocess.run(
            ['/usr/bin/python3', '-I', '-S', '-B', str(capsule / 'launch-cashu-backup.py'),
             digest(manifest), 'stage', str(ciphertext), digest_override, str(receipt),
             str(key), str(destination), '--age', str(selected['age']), '--age-sha256',
             digest(selected['age']), '--statectl', str(selected['statectl']),
             '--key-verifier', str(selected['copy']), '--max-bytes', str(1024**3)],
            stdin=subprocess.DEVNULL, capture_output=True, timeout=180,
            env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'},
        )

    destination = output_root / 'restore-positive'
    outcome = invoke(destination)
    if outcome.returncode:
        raise ValueError('actual restore staging CLI refused original fixture archive')
    result = json.loads(outcome.stdout)
    roles = sorted(('core', 'membership', 'payout', 'signer', 'wallet', 'cashu_signer'))
    if (result['result'] != 'CASHU_RESTORE_STAGED' or result['fenced_roles'] != roles
            or result['keys_verified'] is not True or result['resume_authorized'] is not False
            or result['restore_authorized'] is not False
            or result['external_reconciliation_verified'] is not False
            or result != json.loads((destination / 'STAGED.json').read_bytes())
            or (destination / 'INCOMPLETE').exists() or (destination / 'decrypted.tar').exists()):
        raise ValueError('restore staging outcome differs from expected scope')
    manifest_path = destination / 'FENCED_IMAGES.json'
    image_manifest = json.loads(manifest_path.read_bytes())
    if (digest(manifest_path) != result['fenced_images_sha256']
            or image_manifest['checkpoint_id'] != result['checkpoint_id']
            or sorted(image_manifest['roles']) != roles):
        raise ValueError('staged image manifest differs from receipt')
    for role in roles:
        directory = destination / 'fenced-images' / role
        marker = ('CASHU_RECOVERY_REQUIRED' if role in ('wallet', 'cashu_signer')
                  else 'CYBERHERD_RECOVERY_REQUIRED')
        if ((directory / marker).read_text().strip() != result['checkpoint_id']
                or directory.stat().st_uid != 0 or directory.stat().st_mode & 0o777 != 0o700
                or any(p.stat().st_uid != 0 or p.stat().st_mode & 0o777 != 0o600
                       for p in directory.iterdir())):
            raise ValueError('staged images lack protected ownership or recovery fence')
        for item in image_manifest['roles'][role]:
            image = directory / item['name']
            if digest(image) != item['sha256'] or image.stat().st_size != item['bytes']:
                raise ValueError('staged image differs from retained manifest')
    image = destination / 'fenced-images/wallet' / store.name
    with closing(sqlite3.connect(image.as_uri() + '?immutable=1&mode=ro', uri=True)) as db:
        if db.execute('SELECT value FROM collector_fixture_wal').fetchall() != [(b'synthetic committed WAL value',)]:
            raise ValueError('staged native store lost committed WAL content')
    if any(p.stat().st_mode & 0o111 for p in (destination / 'checkpoint-evidence/programs').iterdir()):
        raise ValueError('stager made an archived program executable')

    def invoke_review(output, *options):
        return subprocess.run(
            ['/usr/bin/python3', '-I', '-S', '-B', str(capsule / 'launch-cashu-backup.py'),
             digest(manifest), 'review', str(destination), digest(destination / 'STAGED.json'),
             '--output', str(output), *options],
            stdin=subprocess.DEVNULL, capture_output=True, timeout=120,
            env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'},
        )

    review_path = output_root / 'empty-restore-worklist.json'
    review = invoke_review(review_path)
    if review.returncode or any(json.loads(review.stdout)['counts'].values()):
        raise ValueError('empty staged checkpoint produced unexpected review work')
    if review_path.stat().st_mode & 0o777 != 0o600:
        raise ValueError('restore worklist is not private')
    report = json.loads(review_path.read_bytes())
    if (report['version'] != 1 or report['staged_receipt_sha256'] != digest(destination / 'STAGED.json')
            or report['reference_receipt_sha256'] is not None
            or report['selection'] != {'group': None, 'after': None, 'limit': 100}):
        raise ValueError('restore worklist omitted its exact source or page binding')
    journal = destination / 'fenced-images/wallet/acquisition.sqlite'
    with closing(sqlite3.connect(journal.as_uri() + '?immutable=1&mode=ro', uri=True)) as db:
        mint, mint_url = db.execute('SELECT mint_id,mint_url FROM cdk_mint_namespaces ORDER BY mint_id LIMIT 1').fetchone()
    custody_selection = output_root / 'empty-custody-selection.json'
    custody = invoke_review(custody_selection, '--custody-mint', mint)
    if custody.returncode or json.loads(custody.stdout)['result'] != 'CASHU_RESTORE_CUSTODY_SELECTED':
        raise ValueError('authenticated launcher did not select an original mint custody namespace')
    custody_file = output_root / 'empty-custody-observation.json'
    observed = subprocess.run([str(selected['copy']), 'observe-restored-custody', str(custody_selection),
                               digest(custody_selection), mint_url, str(custody_file)], stdin=subprocess.DEVNULL,
                              capture_output=True, timeout=15, env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'})
    if observed.returncode or custody_file.stat().st_mode & 0o777 != 0o600:
        raise ValueError('native statectl did not observe the sealed empty custody page')
    custody_receipt = json.loads(custody_file.read_bytes())
    if (custody_receipt['proofs'] or custody_receipt['mint_id'] != mint
            or custody_receipt['total_candidates'] != 0 or custody_receipt['queried_mint'] is not False
            or custody_receipt['resume_authorized'] is not False
            or custody_receipt['selection_sha256'] != digest(custody_selection)):
        raise ValueError('empty custody observer reported a lookup or missing source binding')
    custody_plan = output_root / 'custody-evidence.json'
    custody_plan.write_text(json.dumps({'version': 1, 'treasury': None, 'feeder': None,
        'custody': {'mint_id': mint, 'staged_receipt_sha256': digest(destination / 'STAGED.json'),
                    'observations': [{'file': custody_file.name, 'sha256': digest(custody_file)}]}}))
    custody_plan.chmod(0o600)
    custody_report = output_root / 'empty-custody-comparison.json'
    compared = subprocess.run(
        ['/usr/bin/python3', '-I', '-S', '-B', str(capsule / 'launch-cashu-backup.py'),
         digest(manifest), 'reconcile', str(destination), digest(destination / 'STAGED.json'),
         str(custody_plan), digest(custody_plan), '--output', str(custody_report)],
        stdin=subprocess.DEVNULL, capture_output=True, timeout=120,
        env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'},
    )
    if compared.returncode or custody_report.stat().st_mode & 0o777 != 0o600:
        raise ValueError('authenticated comparison did not accept the complete empty custody observation')
    custody_comparison = json.loads(custody_report.read_bytes())
    if (custody_comparison['custody']['selected_count'] != 0
            or custody_comparison['custody']['observation_sha256'] != [digest(custody_file)]
            or custody_comparison['resume_authorized'] is not False):
        raise ValueError('custody comparison result lost the selected namespace or authority boundary')
    for kind, option in [('strike', '--strike-intent'), ('mint', '--mint-intent'),
                         ('feeder', '--feeder-request')]:
        unknown_selection = output_root / f'unknown-{kind}-selection.json'
        unknown = invoke_review(unknown_selection, option, '00000000-0000-4000-8000-000000000001')
        if (unknown.returncode != 1 or unknown.stdout or unknown_selection.exists()
                or unknown.stderr != b'Cashu restore review refused. Preserve the fenced checkpoint.\n'):
            raise ValueError('authenticated restore launcher selected an absent original operation')
    missing_evidence = output_root / 'missing-original.json'
    missing_evidence.write_text(json.dumps({'version': 1, 'treasury': None,
        'feeder': {'request_id': '00000000-0000-4000-8000-000000000001',
                   'gateway_origin': 'http://127.0.0.1:1/',
                   'observation': {'file': 'absent-provider-output.json', 'sha256': '00' * 32}}}))
    missing_evidence.chmod(0o600)
    comparison_output = output_root / 'missing-original-comparison.json'
    comparison = subprocess.run(
        ['/usr/bin/python3', '-I', '-S', '-B', str(capsule / 'launch-cashu-backup.py'),
         digest(manifest), 'reconcile', str(destination), digest(destination / 'STAGED.json'),
         str(missing_evidence), digest(missing_evidence), '--output', str(comparison_output)],
        stdin=subprocess.DEVNULL, capture_output=True, timeout=120,
        env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'},
    )
    if (comparison.returncode != 1 or comparison.stdout or comparison_output.exists()
            or comparison.stderr != b'Cashu restore reconciliation review refused. Preserve the fenced checkpoint.\n'):
        raise ValueError('actual recovery comparison launcher accepted absent provider evidence')
    # Mutate one disposable stage image after authentication. Review must
    # refuse changed bytes even though its STAGED receipt still matches.
    tampered_output = output_root / 'tampered-worklist.json'
    with image.open('ab') as mutated:
        mutated.write(b'changed')
    try:
        tampered = invoke_review(tampered_output)
        if (tampered.returncode != 1 or tampered.stdout or tampered_output.exists()
                or tampered.stderr != b'Cashu restore review refused. Preserve the fenced checkpoint.\n'):
            raise ValueError('restore review accepted a changed staged image')
    finally:
        with image.open('r+b') as mutated:
            mutated.truncate(image.stat().st_size - len(b'changed'))
    wrong_digest = output_root / 'restore-wrong-digest'
    wrong_key = output_root / 'other-age-identity'
    subprocess.run([str(selected['keygen']), '-o', str(wrong_key)], stdin=subprocess.DEVNULL,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10, check=True)
    failed_decrypt = output_root / 'restore-wrong-identity'
    for attempt in (invoke(wrong_digest, '00' * 32), invoke(destination), invoke(failed_decrypt, key=wrong_key)):
        if (attempt.returncode != 1 or attempt.stdout
                or attempt.stderr != b'Cashu restore staging refused. Preserve any incomplete private stage.\n'):
            raise ValueError('restore staging did not refuse cleanly')
    if (wrong_digest.exists() or not (failed_decrypt / 'INCOMPLETE').is_file()
            or (failed_decrypt / 'fenced-images').exists() or (failed_decrypt / 'STAGED.json').exists()
            or digest(ciphertext) != checksum):
        raise ValueError('failed restore altered source or promoted an incomplete copy')
    return {'actual_authenticated_launcher': True, 'real_age_archive': True,
            'staged_roles': len(roles), 'committed_wal_preserved': True, 'image_manifest_verified': True,
            'refusal_cases': 3, 'archive_programs_executable': False,
            'offline_worklist_launcher_verified': True, 'worklist_tampered_image_refused': True,
            'worklist_source_and_page_bound': True,
            'unknown_strike_selection_refused': True,
            'unknown_mint_and_feeder_selections_refused': True,
            'comparison_missing_evidence_refused': True,
            'empty_custody_observed_by_native_statectl': True,
            'empty_custody_compared_by_authenticated_launcher': True,
            'external_reconciliation_verified': False, 'resume_authorized': False}


def qualify(source,root,states,uids,binaries,configurations,native_policy,args):
    config_root=root/'collector-config'; config_root.mkdir(mode=0o700)
    output_root=root/'collector-output'; output_root.mkdir(mode=0o700)
    selected={}
    for name,path,expected in [('copy',args.collector_copy_cli,args.collector_copy_sha256),
                              ('age',args.age_cli,args.age_sha256),
                              ('keygen',args.age_keygen_cli,args.age_keygen_sha256)]:
        if digest(path)!=expected: raise ValueError('explicit collector fixture artifact differs')
        target=root/('collector-'+name); shutil.copyfile(path,target); target.chmod(0o555)
        if digest(target)!=expected: raise ValueError('collector fixture artifact changed')
        selected[name]=target
    selected['statectl']=root/'statectl'
    selected['tar']=Path('/usr/bin/tar').resolve(strict=True)
    # Retain a committed WAL/SHM set after its only connection has exited.
    # This adds synthetic nonfinancial rows to a disposable actual CDK schema.
    store=next(states['wallet'].glob('mint-*.sqlite'))
    scratch=root/'collector-wal-source.sqlite'
    with closing(sqlite3.connect(store.as_uri()+'?mode=ro',uri=True)) as original, closing(sqlite3.connect(scratch)) as copy:
        original.backup(copy)
    with closing(sqlite3.connect(scratch)) as writer:
        writer.execute('PRAGMA journal_mode=WAL');writer.execute('PRAGMA wal_autocheckpoint=0')
        writer.execute('PRAGMA wal_checkpoint(TRUNCATE)')
        writer.execute('CREATE TABLE collector_fixture_wal(value BLOB NOT NULL)')
        writer.execute('INSERT INTO collector_fixture_wal VALUES(?)',[b'synthetic committed WAL value'])
        writer.commit()
        retained={suffix:Path(str(scratch)+suffix).read_bytes() for suffix in ('','-wal','-shm')}
    for suffix,raw in retained.items():
        target=Path(str(store)+suffix);target.write_bytes(raw);target.chmod(0o600)
        os.chown(target,uids['wallet'],uids['wallet'])
    identity=config_root/'disposable-backup-identity'
    created=subprocess.run([str(selected['keygen']),'-o',str(identity)],stdin=subprocess.DEVNULL,
        capture_output=True,timeout=10)
    if created.returncode: raise ValueError('synthetic recovery identity creation failed')
    recipient=subprocess.check_output([str(selected['keygen']),'-y',str(identity)],text=True).strip()
    native_roles={'core':'core','membership':'membership','payout':'payout','signer':'payout_signer',
                  'wallet':'wallet','cashu_signer':'signer'}
    workers={role:{'uid':uids[key],'gid':uids[key],'state':str(states[key]),
        'unit':native_policy['components'][role]['unit'] if role in native_policy['components']
               else 'lg-cashu-collector-fixture-'+role+'.service'} for role,key in native_roles.items()}
    cashu_client=config_root/'cashu-client.hex'; cashu_client.write_text('0'*63+'2');cashu_client.chmod(0o600)
    payout_client=config_root/'payout-client.hex'; payout_client.write_text('0'*63+'5');payout_client.chmod(0o600)
    policy={'version':1,'operator_exclusive':True,'workers':workers,
        'native_policy':str(root/'recovery/maintenance-policy.toml'),
        'core_config':str(configurations['core']),'membership_config':str(configurations['membership']),
        'payout_config':str(configurations['payout']),'payout_signer_config':str(configurations['payout_signer']),
        'wallet_config':str(configurations['wallet']),'cashu_signer_config':str(configurations['signer']),
        'cashu_client_source':str(cashu_client),'payout_client_source':str(payout_client),
        'historical_receiving_keys':[],'extra_files':[],
        'artifacts':{name:{'path':str(path),'sha256':digest(path)} for name,path in selected.items() if name!='keygen'},
        'runtime_artifacts':{role:{'path':str(binaries[key]),'sha256':digest(binaries[key])} for role,key in native_roles.items()},
        'recipient':recipient,'component_bytes':128*1024**2,'total_bytes':1024**3,'deadline_seconds':300}
    policy_path=config_root/'policy.json'
    policy_path.write_text(json.dumps(policy));policy_path.chmod(0o600)
    module=runpy.run_path(str(source/'deploy/scripts/collect-cashu-backup.py'),
        init_globals={'verified_tool_identity':lambda:{'fixture':True,'authenticated_launcher_executed':False}})
    collect=module['collect']; globals_=collect.__globals__
    globals_['GLOBAL_LOCK']=config_root/'maintenance.lock'
    check=module['check_quiescence']
    original_run=subprocess.run
    active=False
    calls=0

    def service_status(command,**kwargs):
        nonlocal calls
        if command[:2]!=['/usr/bin/systemctl','show']:
            raise ValueError('fixture only mocks systemctl show')
        calls+=1; unit=command[-1]
        if unit not in {w['unit'] for w in workers.values()}: raise ValueError('unexpected service identity')
        return types.SimpleNamespace(returncode=0,stdout=(f'Id={unit}\nLoadState=masked\n'
            f'ActiveState={"active" if active else "inactive"}\nSubState=dead\nMainPID=0\nControlPID=0\n'
            'UnitFileState=masked\nControlGroup=\n').encode())

    def checked_status(workers,budget):
        with patch.object(subprocess,'run',side_effect=service_status):
            check(workers,budget)
    globals_['check_quiescence']=checked_status
    original={str(p):digest(p) for directory in states.values() for p in directory.rglob('*') if p.is_file()}
    destination=output_root/'positive'
    result=collect(policy_path,digest(policy_path),destination)
    if (result['result']!='CASHU_COORDINATED_BACKUP_COLLECTED' or not result['quiescence_verified']
            or result['restore_authorized'] or result['resume_authorized']
            or result['recovery_key_possession_verified'] or not result['plaintext_retained']
            or (destination/'INCOMPLETE').exists() or not (destination/'COMPLETE.json').exists()):
        raise ValueError('collector result/marker scope differs')
    ciphertext=destination/'checkpoint.tar.age'
    if digest(ciphertext)!=result['ciphertext_sha256']:raise ValueError('ciphertext receipt differs')
    collection=json.loads((destination/'plaintext/collection.json').read_bytes())
    for item in collection['originals']:
        if digest(destination/'plaintext'/item['path'])!=item['sha256']:
            raise ValueError('retained copy digest differs from collection metadata: '+item['path'])
    restored=root/'collector-decrypted.tar'
    with restored.open('xb') as output:
        os.fchmod(output.fileno(),0o600)
        opened=original_run([str(selected['age']),'--decrypt','-i',str(identity),str(ciphertext)],
            stdout=output,stderr=subprocess.PIPE,stdin=subprocess.DEVNULL,timeout=60)
    if opened.returncode:raise ValueError('actual collected archive decryption failed')
    with tarfile.open(restored,'r:') as archive:
        members={m.name:m for m in archive.getmembers()}
        if any(m.issym() or m.islnk() or not(m.isfile() or m.isdir()) for m in members.values()):
            raise ValueError('unexpected archive entry type')
        for path in (destination/'plaintext').rglob('*'):
            if not path.is_file():continue
            name=str(path.relative_to(destination/'plaintext'))
            if name not in members or digest(path)!=hashlib.sha256(archive.extractfile(members[name]).read()).hexdigest():
                raise ValueError('encrypted archive omitted or changed a retained component')
    after={str(p):digest(p) for directory in states.values() for p in directory.rglob('*') if p.is_file()}
    if original!=after:raise ValueError('collector changed original application bytes')
    restore_result = qualify_restore(source, output_root, destination, identity, selected, store)
    key_stage=destination/'plaintext/recovery-material'
    key_negatives=0
    def key_check(success):
        outcome=original_run([str(selected['copy']),'verify-recovery-keys',str(key_stage)],
            stdin=subprocess.DEVNULL,capture_output=True,timeout=3)
        if (outcome.returncode==0)!=success:raise ValueError('unexpected native recovery-key outcome')
        if not success and (outcome.stdout or outcome.stderr!=b'Recovery key verification refused; preserve the incomplete private stage.\n'):
            raise ValueError('recovery-key refusal disclosed private context')
    for name,replacement in [('wallet-seed.bin',bytes([9])*64),('receiving-key.bin',bytes(32)),
                             ('cashu-client.hex',('0'*63+'5').encode()),
                             ('payout-client.hex',('0'*63+'2').encode())]:
        path=key_stage/name; saved=path.read_bytes()
        try:path.write_bytes(replacement);key_check(False);key_negatives+=1
        finally:path.write_bytes(saved)
    bindings_path=key_stage/'key-bindings.json';saved_bindings=bindings_path.read_bytes()
    for field,name,secret,public in [
        ('payout_client','payout-client.hex','2',json.loads(saved_bindings)['cashu_client']),
        ('cashu_client','cashu-client.hex','3',json.loads(saved_bindings)['owner'])]:
        path=key_stage/name;saved=path.read_bytes();changed=json.loads(saved_bindings);changed[field]=public
        try:
            path.write_text('0'*63+secret);bindings_path.write_text(json.dumps(changed))
            key_check(False);key_negatives+=1
        finally:path.write_bytes(saved);bindings_path.write_bytes(saved_bindings)
    cashu_key=key_stage/'cashu-client.hex';saved=cashu_key.read_bytes()
    try:cashu_key.write_bytes(saved+b'\n');key_check(True)
    finally:cashu_key.write_bytes(saved)
    # Exercise real output-file limits and deterministic process timeout without
    # repeating database initialization or changing application state.
    fault=root/'collector-pipeline-faults';fault.mkdir(mode=0o700)
    fault_plain=fault/'plain';fault_plain.mkdir(mode=0o700)
    payload=fault_plain/'synthetic';payload.write_bytes(b'synthetic checkpoint\0'*32768);payload.chmod(0o600)
    before_fault=digest(payload)
    marker=fault/'INCOMPLETE';marker.write_bytes(b'retain interrupted pipeline');marker.chmod(0o600)
    pipeline=module['encrypt'];budget_class=module['Budget']
    try:pipeline(fault_plain,fault/'size-limited.age',selected,recipient,budget_class(payload.stat().st_size+256,5))
    except ValueError:pass
    else:raise ValueError('actual ciphertext size limit was ignored')
    if not(fault/'size-limited.age').exists() or (fault/'size-limited.age').stat().st_size>256:
        raise ValueError('bounded interrupted ciphertext was not retained')
    stall=fault/'stall';stall.write_text('#!/bin/sh\nexec /usr/bin/sleep 60\n');stall.chmod(0o500)
    started=time.monotonic()
    try:pipeline(fault_plain,fault/'timed-out.age',{**selected,'age':stall},recipient,budget_class(4*1024**2,0.2))
    except ValueError:pass
    else:raise ValueError('stalled encryption process was not bounded')
    if time.monotonic()-started>5 or not marker.exists() or digest(payload)!=before_fault:
        raise ValueError('pipeline fault changed retained plaintext or failed to stop')
    negative=0
    active=True
    try:collect(policy_path,digest(policy_path),output_root/'active-writer')
    except ValueError:negative+=1
    else:raise ValueError('active writer was accepted')
    if (output_root/'active-writer').exists():raise ValueError('active-writer refusal reached copying')
    active=False
    try:collect(policy_path,'aa'*32,output_root/'wrong-policy')
    except ValueError:negative+=1
    else:raise ValueError('wrong reviewed policy hash accepted')
    try:collect(policy_path,digest(policy_path),destination)
    except FileExistsError:negative+=1
    else:raise ValueError('preexisting destination overwritten')
    # A valid but wrong seed must refuse even though all database inventory
    # hashes and registrations still agree. Keep the failed raw/copy evidence.
    seed=states['wallet']/'wallet-seed.bin'; retained=seed.read_bytes()
    seed.write_bytes(bytes([9])*64)
    try:
        try:collect(policy_path,digest(policy_path),output_root/'wrong-seed')
        except ValueError:negative+=1
        else:raise ValueError('wrong seed was accepted with a matching epoch')
        if not(output_root/'wrong-seed/INCOMPLETE').exists():raise ValueError('failed checkpoint evidence lost')
        if (output_root/'wrong-seed/COMPLETE.json').exists():raise ValueError('failed checkpoint marked complete')
    finally:seed.write_bytes(retained)
    return {'result':'CASHU_COORDINATED_COLLECTOR_FIXTURE_VERIFIED',
        'native_database_inventory_and_correlations':True,'native_key_bindings':True,
        'actual_age_archive_roundtrip':True,'original_application_bytes_unchanged':True,
        'committed_wal_copy_hashes_verified':True,
        'refusal_cases':negative,'systemctl_calls_mocked':calls,'proc_uid_scan_actual':True,
        'native_key_material_refusal_cases':key_negatives,'client_credential_optional_lf_accepted':True,
        'encryption_output_limit_refused':True,'encryption_timeout_stub_refused':True,
        'maintenance_lock_path_is_disposable':True,'authenticated_launcher_executed':False,
        'empty_native_state_only':True,'funded_restore_qualified':False,
        'installed_or_live_acceptance':False,'production_or_physical_side_effects':False,
        'restore_staging': restore_result}
