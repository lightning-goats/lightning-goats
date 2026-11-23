"""Read original synthetic provider outcomes for two authenticated stages."""
import json
import os
from pathlib import Path
import runpy
import subprocess
import time
from urllib.parse import urlsplit


def compare_funded_providers(capsule, bootstrap, stage, reference, source, output, core_cli, common):
    output.mkdir(mode=0o700)
    exchange = source / 'exchange'

    def read(path):
        return json.loads(path.read_bytes())

    def sha(path):
        return common['sha'](path.read_bytes())

    stage_sha, reference_sha = (sha(path / 'STAGED.json') for path in (stage, reference))
    launcher = ['/usr/bin/python3', '-I', '-S', '-B', str(capsule / 'launch-cashu-backup.py'), bootstrap]
    preserved = {path: sha(path) for directory in (stage, reference)
                 for path in (directory / 'STAGED.json', directory / 'FENCED_IMAGES.json',
                              *(p for p in (directory / 'fenced-images').rglob('*') if p.is_file()))}
    installed = output.parent / 'installed-fenced-runtime'
    for role in (stage / 'fenced-images').iterdir():
        copies = {path.name: sha(path) for path in (installed / role.name).iterdir()}
        if copies != {path.name: preserved[path] for path in role.iterdir()}:
            raise ValueError('installed fenced bytes differ from the selected stage')
        preserved.update({installed / role.name / name: digest for name, digest in copies.items()})

    def run(*command, rejected=False):
        result = subprocess.run(list(map(str, command)), stdin=subprocess.DEVNULL, capture_output=True,
                                timeout=25, env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'})
        if rejected:
            if (result.returncode != 1 or result.stdout or result.stderr !=
                    b'Cashu restore reconciliation review refused. Preserve the fenced checkpoint.\n'):
                raise ValueError('authenticated comparator accepted an incomplete or rebound observation batch')
        elif result.returncode:
            raise ValueError(f'funded fixture observation failed: {result.stderr.decode()[-1200:]}')

    def select(name, selected, checksum, *options):
        path = output / (name + '.json')
        run(*launcher, 'review', selected, checksum, '--output', path, *options)
        return path

    worklist = read(select('reference-worklist', stage, stage_sha, '--reference', reference,
                           '--reference-sha256', reference_sha))['records']
    mint_rows = [row['reference'] for row in worklist['mint_melts']['items']]
    strike_rows = {row['reference']['id']: row for row in worklist['strike_receives']['items']}
    feeder_rows = worklist['feeder_requests']['items']
    plan_rows = worklist['financial_feed_plans']['items']
    later_credit = {'id': read(exchange / 'reference-credit.json'), 'mint_id': 'fixture-a',
                    'state': 'credited', 'net': 16}
    if (len(mint_rows) != 2 or len(strike_rows) != 2 or len(feeder_rows) != 1
            or {row['reference']['mint_id'] for row in strike_rows.values()} != {'fixture-a', 'fixture-b'}
            or set(strike_rows) != {row['id'] for row in mint_rows}
            or worklist['mint_splits']['total'] or len(plan_rows) != 1
            or worklist['cashu_acquisitions']['items'] != [{'staged': None, 'reference': later_credit}]):
        raise ValueError('funded worklist differs from the original melts, feed and later credit')

    selections, strikes = {}, {}
    credential = output / 'synthetic-strike-key'
    common['write'](credential, b'receive-only-test-key')
    for row in mint_rows:
        intent = row['id']
        strike = strike_rows[intent]
        mint = strike['reference']['mint_id']
        custody = select(mint + '-custody-selection', stage, stage_sha, '--custody-mint', mint)
        candidate = select(mint + '-reference-custody-selection', reference, reference_sha, '--custody-mint', mint)
        melt = select(mint + '-melt-selection', reference, reference_sha, '--mint-intent', intent)
        selections[mint] = {'custody': custody.read_text(), 'reference_custody': candidate.read_text(),
                            'melt': melt.read_text()}
        if strike['staged'] is not None:
            strike_path, strike_sha = stage, stage_sha
        else:
            strike_path, strike_sha = reference, reference_sha
        strike_selection = select(mint + '-strike-selection', strike_path, strike_sha,
                                  '--strike-intent', intent, '--receive-id', strike['reference']['receive_id'])
        origin = read(strike_selection)['issued']['intent']['destination']['origin']
        url = urlsplit(origin)
        if url.scheme != 'http' or url.hostname != '127.0.0.1' or not url.port:
            raise ValueError('expected the original isolated Strike fixture listener')
        evidence = output / (mint + '-strike-observation.json')
        run(core_cli, '--config', output / 'unused.toml', 'verify-restored-cashu-strike', '--selection', strike_selection,
            '--selection-sha256', sha(strike_selection), '--strike-origin', origin,
            '--credential', credential, '--output', evidence)
        strikes[mint] = {'file': evidence.name, 'sha256': sha(evidence)}

    # Use the live original synthetic mints for the read-only operations. The
    # native observer receives the exact protected CLI selection bytes.
    request = exchange / 'restore-mint-selections.pending'
    common['write_json'](request, selections)
    os.chown(request, 0, exchange.stat().st_gid)
    request.chmod(0o640)
    request.rename(exchange / 'restore-mint-selections.json')
    minted = exchange / 'restore-mint-observations.json'
    deadline = time.monotonic() + 30
    while not minted.exists():
        if time.monotonic() >= deadline:
            raise ValueError('original synthetic mint observers did not finish')
        time.sleep(0.025)
    mint_reports = read(minted)
    if set(mint_reports) != set(selections):
        raise ValueError('observations omit an original mint')

    def retain(name, report):
        path = output / (name + '.json')
        common['write_json'](path, report)
        return {'file': path.name, 'sha256': sha(path)}

    custody, reference_custody, treasury = {}, {}, []
    for row in mint_rows:
        mint = strike_rows[row['id']]['reference']['mint_id']
        reports = mint_reports[mint]
        custody[mint] = [retain(mint + '-custody-observation', reports['custody'])]
        reference_custody[mint] = [retain(mint + '-reference-custody-observation', reports['reference_custody'])]
        treasury.append({'intent_id': row['id'], 'strike': strikes[mint],
                         'mint': retain(mint + '-melt-observation', reports['melt'])})
    feeder = feeder_rows[0]['reference']['id']
    earlier_plan, later_plan = plan_rows[0]['staged'], plan_rows[0]['reference']
    if (feeder_rows[0]['staged'] is not None or read(exchange / 'credit-consumed.json') != feeder
            or earlier_plan['state'] != 'pending' or earlier_plan['request_id'] is not None
            or later_plan['state'] != 'dispatched' or later_plan['request_id'] != feeder
            or earlier_plan['id'] != later_plan['id']
            or earlier_plan['allocation_sha256'] != later_plan['allocation_sha256']):
        raise ValueError('reference feeder or financial plan differs from the original confirmation')
    feeder_selection = read(select('feeder-selection', reference, reference_sha, '--feeder-request', feeder))
    fixture = runpy.run_path(str(Path(__file__).with_name('test-cashu-combined-settlement.py')))
    observed = json.loads(fixture['verify_feeder_read'](feeder_selection, core_cli)['confirmed'])
    feeder_entry = {'request_id': feeder, 'gateway_origin': observed['gateway_origin'],
                    'observation': retain('feeder-observation', observed)}
    plan = {'version': 3, 'treasury': treasury, 'feeder': [feeder_entry],
            'custody_all': {'staged_receipt_sha256': stage_sha, 'mints': custody},
            'reference_custody_all': {'staged_receipt_sha256': reference_sha, 'mints': reference_custody}}

    def compare(name, evidence, rejected=False):
        manifest = retain(name + '-manifest', evidence)
        result = output / (name + '-result.json')
        run(*launcher, 'reconcile', stage, stage_sha, output / manifest['file'], manifest['sha256'],
            '--reference', reference, '--reference-sha256', reference_sha, '--output', result,
            rejected=rejected)
        if rejected and result.exists():
            raise ValueError('rejected observations produced a comparison')
        return None if rejected else read(result)

    report = compare('funded-restore', plan)
    compare('omitted-operation', {**plan, 'feeder': []}, rejected=True)
    compare('omitted-reference-custody', {key: value for key, value in plan.items() if key != 'reference_custody_all'},
            rejected=True)
    compare('omitted-reference-mint', {**plan, 'reference_custody_all': {**plan['reference_custody_all'],
            'mints': {'fixture-a': reference_custody['fixture-a']}}}, rejected=True)
    compare('rebound-custody', {**plan, 'reference_custody_all': {**plan['reference_custody_all'],
            'mints': custody}}, rejected=True)
    batch = report['custody_all']
    later = report['reference_custody_all']
    if (report['staged_receipt_sha256'] != stage_sha or report['reference_receipt_sha256'] != reference_sha
            or batch['mint_spent_sats'] != 400 or batch['stale_local_unspent_sats'] != 400
            or batch['mint_unspent_sats'] or batch['mint_unresolved_sats']
            or {mint: item['stale_local_unspent_sats'] for mint, item in batch['mints'].items()}
            != {'fixture-a': 250, 'fixture-b': 150}
            or later['staged_receipt_sha256'] != reference_sha or set(later['mints']) != set(batch['mints'])
            or {mint: item['mint_unspent_sats'] for mint, item in later['mints'].items()}
            != {'fixture-a': 16, 'fixture-b': 0}
            or later['mint_unspent_sats'] != 16 or later['selected_count'] == 0
            or any(later[key] for key in ('stale_local_unspent_sats', 'mint_unresolved_sats', 'mint_unspent_held_sats'))
            or any(report['reference_pending_counts'].values())
            or sorted(row['deposit_sats'] for row in report['treasury']) != [148, 248]
            or any(not row['observed_pair_matches'] or not row['paid_observation_missing_from_stage']
                   or row['reference_accounting']['completion_totals'] is None for row in report['treasury'])
            or len(report['feeder']) != 1 or not report['feeder'][0]['confirmed_observation_missing_from_stage']
            or report['feeder'][0]['checkpoint_conflict'] or report['resume_authorized']
            or any(sha(path) != expected for path, expected in preserved.items())):
        raise ValueError('funded comparison changed state or lost original provider evidence')
    return {'staged_receipt_sha256': stage_sha, 'reference_receipt_sha256': reference_sha,
            'stale_installed_custody_sats': batch['stale_local_unspent_sats'],
            'reference_mints_observed': later['mint_namespaces_compared'],
            'reference_proof_points_compared': later['selected_count'],
            'reference_mint_unspent_sats': later['mint_unspent_sats'],
            'reference_mint_spent_sats': later['mint_spent_sats'],
            'reference_mint_unresolved_sats': later['mint_unresolved_sats'],
            'stale_reference_custody_sats': later['stale_local_unspent_sats'],
            'reference_credit_retained_sats': later_credit['net'],
            'reference_pending_counts': report['reference_pending_counts'],
            'original_financial_plan_dispatched_with_same_allocation': True,
            'original_mints_observed': batch['mint_namespaces_compared'], 'original_strike_receives_verified': 2,
            'original_strike_deposit_sats': sum(row['deposit_sats'] for row in report['treasury']),
            'original_feeder_confirmation_verified': True, 'omitted_operation_refused': True,
            'omitted_reference_custody_refused': True, 'omitted_reference_mint_refused': True,
            'rebound_custody_refused': True, 'checkpoint_bytes_unchanged': True, 'resume_authorized': False}
