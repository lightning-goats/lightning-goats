#!/usr/bin/env python3
"""Compare protected provider observations with the exact fenced journals."""
import argparse
from contextlib import closing, ExitStack
import json
from pathlib import Path
import re
import runpy
import sqlite3
import uuid

HERE = Path(__file__).resolve().parent
REVIEW = runpy.run_path(str(HERE / 'review-cashu-restore.py'))
NO_AUTHORITY = {'external_reconciliation_verified': False, 'restore_authorized': False,
                'resume_authorized': False}
RESULTS = {'mint': 'CASHU_RESTORE_MINT_MELT_OBSERVED', 'strike': 'CASHU_RESTORE_STRIKE_RECEIVE_VERIFIED',
           'feeder': 'CASHU_RESTORE_FEEDER_REQUEST_OBSERVED', 'custody': 'CASHU_RESTORE_MINT_CUSTODY_OBSERVED'}
CUSTODY_TOTALS = ('mint_unspent_sats', 'mint_spent_sats', 'mint_unresolved_sats', 'stale_local_unspent_sats')
BATCH_LIMIT = 1000


def require(condition, message):
    if not condition:
        raise ValueError(message)


def valid_id(value):
    parsed = uuid.UUID(value)
    require(parsed.int and str(parsed) == value, 'invalid original UUID')
    return value


def read_bound(path, expected, maximum, common, budget):
    common['protected_directory'](path.parent, private=True)
    require(re.fullmatch('[0-9a-f]{64}', expected), 'invalid evidence digest')
    with closing(common['Frozen'](path, 0, maximum, budget)) as source:
        require(source.digest == expected, 'evidence bytes differ from reviewed digest')
        source.file.seek(0)
        raw = source.file.read(maximum + 1)
        require(len(raw) <= maximum and common['sha'](raw) == expected, 'evidence changed during read')
        source.check()
    return common['document'](raw)


def observation_item(parent, entry, stages, kind, common, budget):
    require(set(entry) == {'file', 'sha256'}, 'invalid observation evidence fields')
    member = REVIEW['RESTORE']['relative_path'](entry['file'])
    require(len(member.parts) == 1, 'observation file must be beside evidence manifest')
    report = read_bound(parent / member, entry['sha256'], 2 * 1024 * 1024, common, budget)
    picked = stages[report['staged_receipt_sha256']]
    require(type(report['version']) is int and report['version'] == 1 and report['result'] == RESULTS[kind]
            and report['checkpoint_id'] == picked[0] and type(report['observed_at']) is int
            and report['observed_at'] >= 0 and all(report[k] is False for k in NO_AUTHORITY),
            'observation is outside the reviewed checkpoints or scope')
    return report, picked


def matches_selection(report, selection, common):
    require(report['selection_sha256'] == common['sha'](common['json_bytes'](selection)),
            'observation does not bind the exact retained original selection')


def mint_status(report, selection, common):
    issued, quote, outcome = selection['issued'], selection['quote'], report['observation']
    state = outcome['state']
    require(report['intent_id'] == issued['intent']['id'] and report['mint_id'] == issued['intent']['mint_id']
            and report['operation_id'] == selection['operation_id'] and report['mint_url'] == quote['mint_url']
            and outcome['quote'] == quote['id'] and outcome['method'] == quote['payment_method']
            and outcome.get('request') in (None, issued['invoice']) and outcome.get('unit') in (None, quote['unit'])
            and all(type(outcome[k]) is int and outcome[k] == quote[k] for k in ('amount', 'fee_reserve', 'expiry'))
            and state in ('PAID', 'PENDING', 'UNPAID', 'UNKNOWN', 'FAILED'),
            'mint observation contradicts original quote')
    preimage = outcome.get('payment_preimage')
    if state == 'PAID':
        require(report['paid_preimage_verified'] is True and isinstance(preimage, str)
                and re.fullmatch('[0-9a-f]{64}', preimage) and common['sha'](bytes.fromhex(preimage)) == issued['payment_hash'],
                'paid mint observation lacks the original preimage')
    else:
        require(report['paid_preimage_verified'] is False and preimage is None and not outcome.get('change'),
                'unresolved mint observation contains paid evidence')
    return state


def btc_amount_matches(amount, msat):
    if amount['currency'] != 'BTC' or not re.fullmatch(r'[0-9]+(?:\.[0-9]{0,11})?', amount['amount'].strip()):
        return False
    whole, _, fraction = amount['amount'].strip().partition('.')
    expected_whole, expected_fraction = divmod(msat, 100_000_000_000)
    return (whole.lstrip('0') or '0') == str(expected_whole) and int(fraction.ljust(11, '0')) == expected_fraction


def strike_matches(report, selection):
    issued, outcome = selection['issued'], report['observation']
    msat = issued['intent']['deposit_sats'] * 1000
    require(report['intent_id'] == issued['intent']['id'] and report['intent_digest'] == selection['intent_digest']
            and outcome['receive_id'] == selection['receive_id'] and outcome['payment_hash'] == issued['payment_hash']
            and outcome['receive_request_id'] == issued['receive_request_id'] and outcome['receive_type'] == 'LIGHTNING'
            and type(outcome['amount_msat']) is int and outcome['amount_msat'] == msat
            and btc_amount_matches(outcome['received'], msat)
            and (outcome['credited'] is None or btc_amount_matches(outcome['credited'], msat)),
            'Strike observation differs from original BTC receive')


def retained_settlement(core, wallet, selected, mint_selected, mint_report, common):
    intent = selected['issued']['intent']
    id = intent['id']
    decode = common['document']
    deposit = core.execute('SELECT receive_id,payment_hash,deposit_sats FROM cashu_treasury_deposits WHERE intent_id=?',
                           (id,)).fetchone()
    if deposit is not None:
        require(tuple(deposit) == (selected['receive_id'], selected['issued']['payment_hash'], intent['deposit_sats']),
                'retained Strike deposit disagrees with observed original')
    totals = core.execute('SELECT input_sats,deposit_sats,fee_sats,change_sats FROM cashu_treasury_completions WHERE intent_id=?',
                          (id,)).fetchone()
    core_row = core.execute('SELECT result_json FROM cashu_treasury_wallet_results WHERE intent_id=?', (id,)).fetchone()
    wallet_row = wallet.execute('SELECT report,paid_evidence FROM treasury_results WHERE intent_id=?', (id,)).fetchone()
    wallet_report = decode(wallet_row[0]) if wallet_row is not None else None
    cleanup = wallet.execute('SELECT operation_id,report_digest FROM treasury_cdk_cleanup WHERE intent_id=?', (id,)).fetchone()
    if cleanup is not None:
        require(wallet_row is not None and tuple(cleanup) == (
            mint_selected['operation_id'], bytes.fromhex(common['sha'](wallet_row[0]))),
            'retained CDK cleanup differs from the original wallet report')
    if wallet_report is not None:
        require(wallet_report['intent_id'] == id and wallet_report['intent_digest'] == selected['intent_digest']
                and wallet_report['quote_id'] == mint_selected['quote']['id'],
                'retained wallet result differs from observed quote')
        paid = decode(wallet_row['paid_evidence'])
        require(mint_status({**mint_report, 'observation': paid, 'paid_preimage_verified': True}, mint_selected, common) == 'PAID',
                'retained wallet result lacks paid evidence for the original mint quote')
    if core_row is not None:
        require(wallet_report is not None and decode(core_row[0]) == wallet_report,
                'retained core and wallet settlement reports differ')
    if totals is not None:
        require(wallet_report is not None and core_row is not None and deposit is not None,
                'retained completion lacks a matching deposit or wallet report')
        totals = dict(totals)
        fields = ('input_sats', 'deposit_sats', 'fee_sats', 'change_sats')
        amounts = tuple(totals[k] for k in fields)
        rows = core.execute('SELECT ledger_id,input_sats,deposit_sats,fee_sats,change_sats '
                            'FROM cashu_treasury_source_results WHERE intent_id=? ORDER BY ledger_id LIMIT 129', (id,))
        parts = [dict(row) for row in rows]
        require(parts == wallet_report['sources'] and len(parts) <= 128
                and [(p['ledger_id'], p['input_sats']) for p in parts]
                == [(p['ledger_id'], p['input_sats']) for p in intent['sources']]
                and amounts == tuple(sum(p[k] for p in parts) for k in fields)
                and amounts[0] == intent['input_sats'] and amounts[1] == intent['deposit_sats']
                and amounts[0] == sum(amounts[1:]) and 0 <= amounts[2] <= intent['maximum_fee_sats'],
                'retained settlement and allocation totals disagree')
    return {'strike_deposit_retained': deposit is not None, 'wallet_report_retained': wallet_report is not None,
            'core_wallet_report_retained': core_row is not None, 'wallet_cdk_cleanup_retained': cleanup is not None,
            'completion_totals': totals}


def compare_treasury(entry, parent, stages, staged_sha, reference_sha, common, budget):
    require(set(entry) == {'intent_id', 'strike', 'mint'}, 'invalid treasury evidence fields')
    original = valid_id(entry['intent_id'])
    strike, strike_stage = observation_item(parent, entry['strike'], stages, 'strike', common, budget)
    mint, mint_stage = observation_item(parent, entry['mint'], stages, 'mint', common, budget)
    decode = common['document']
    selected = REVIEW['select_strike'](strike_stage[1], strike_stage[0], strike['staged_receipt_sha256'],
                                       original, valid_id(strike['observation']['receive_id']), decode)
    mint_selected = REVIEW['select_mint'](mint_stage[2], mint_stage[0], mint['staged_receipt_sha256'], original, decode)
    matches_selection(strike, selected, common)
    matches_selection(mint, mint_selected, common)
    require(selected['issued'] == mint_selected['issued'] and strike['intent_digest'] == mint['intent_digest'],
            'mint and Strike observations refer to different original invoices')
    strike_matches(strike, selected)
    state = mint_status(mint, mint_selected, common)

    def retained(sha):
        _, core, wallet = stages[sha]
        return retained_settlement(core, wallet, selected, mint_selected, mint, common)

    staged = retained(staged_sha)
    reference = retained(reference_sha) if reference_sha is not None else None
    return {'intent_id': original, 'quote_id': mint_selected['quote']['id'], 'mint_state': state,
            'receive_id': selected['receive_id'], 'deposit_sats': selected['issued']['intent']['deposit_sats'],
            'observed_pair_matches': state == 'PAID', 'staged_accounting': staged, 'reference_accounting': reference,
            'paid_observation_missing_from_stage': state == 'PAID' and staged['completion_totals'] is None,
            'observation_sha256': {'mint': entry['mint']['sha256'], 'strike': entry['strike']['sha256']}}


def compare_feeder(entry, parent, stages, staged_sha, reference_sha, common, budget):
    require(set(entry) == {'request_id', 'gateway_origin', 'observation'}, 'invalid feeder evidence fields')
    original = valid_id(entry['request_id'])
    report, report_stage = observation_item(parent, entry['observation'], stages, 'feeder', common, budget)
    selected = REVIEW['select_feeder'](report_stage[1], report_stage[0], report['staged_receipt_sha256'], original)
    matches_selection(report, selected, common)
    observed = report['observation']
    status = observed['status']
    require(report['gateway_origin'] == entry['gateway_origin'] and observed['request_id'] == original
            and status in ('not_dispatched', 'pending', 'confirmed', 'ambiguous')
            and report['threshold_sats'] == selected['threshold_sats']
            and report['checkpoint_status'] == selected['checkpoint_status'],
            'gateway observation differs from the original feeder request')
    refusal = observed.get('refusal')
    if status == 'not_dispatched':
        require(refusal is not None and refusal['reason'] in ('safety', 'unresolved', 'capacity')
                and type(refusal['retry_after_seconds']) is int and 1 <= refusal['retry_after_seconds'] <= 86400,
                'gateway non-dispatch observation lacks a valid refusal')
    else:
        require(refusal is None, 'gateway observation contains contradictory refusal information')

    def retained(sha):
        row = stages[sha][1].execute('SELECT status,threshold_sats FROM feed_attempts WHERE id=?', (original,)).fetchone()
        if row is None:
            return None
        local, threshold = row
        require(threshold == selected['threshold_sats'], 'retained feeder threshold differs')
        return local

    def conflicts(local):
        return (local, status) in (('confirmed', 'not_dispatched'), ('reconciled_not_fed', 'confirmed'))

    require(report['checkpoint_conflict'] is conflicts(selected['checkpoint_status']),
            'provider observation conflict flag contradicts its selected stage')
    staged = retained(staged_sha)
    reference = retained(reference_sha) if reference_sha is not None else None
    return {'request_id': original, 'gateway_status': status, 'staged_status': staged, 'reference_status': reference,
            'checkpoint_conflict': conflicts(staged) or conflicts(reference),
            'confirmed_observation_missing_from_stage': status == 'confirmed' and staged != 'confirmed',
            'observation_sha256': entry['observation']['sha256']}


def custody_amounts(selected, report):
    observations = report['proofs']
    require(len(observations) == len(selected) and report['queried_mint'] is bool(selected),
            'custody report does not cover its original page')
    totals = dict.fromkeys(CUSTODY_TOTALS, 0)
    unspent_held = 0
    for original, observed in zip(selected, observations):
        require(type(observed['amount_sats']) is int and all(observed[k] == value for k, value in original.items()),
                'custody observation differs from original proof')
        state = observed['mint_state']
        require(state in ('SPENT', 'UNSPENT', 'RESERVED', 'PENDING', 'PENDING_SPENT'), 'unknown mint proof state')
        key = {'SPENT': 'mint_spent_sats', 'UNSPENT': 'mint_unspent_sats'}.get(state, 'mint_unresolved_sats')
        totals[key] += original['amount_sats']
        if state == 'SPENT' and original['local_state'] == 'UNSPENT':
            totals['stale_local_unspent_sats'] += original['amount_sats']
        if state == 'UNSPENT' and original['local_state'] != 'UNSPENT':
            unspent_held += original['amount_sats']
    require(all(type(report[k]) is int and report[k] == amount for k, amount in totals.items()),
            'custody observation totals disagree')
    return {**totals, 'mint_unspent_held_sats': unspent_held}


def compare_custody(entry, parent, stages, mint_source, common, budget):
    require(set(entry) == {'mint_id', 'staged_receipt_sha256', 'observations'}
            and isinstance(entry['observations'], list) and entry['observations'], 'invalid custody evidence fields')
    stage_sha, mint = entry['staged_receipt_sha256'], entry['mint_id']
    checkpoint = stages[stage_sha][0]
    wallet, store_sha = mint_source
    after, count, observations, totals = None, 0, [], {}
    first_at, last_at = None, None
    with REVIEW['open_custody_store'](wallet, mint, store_sha, common, budget) as (store, mint_url):
        total = REVIEW['custody_total'](store, mint_url)
        for offset, entry_item in enumerate(entry['observations']):
            require(offset == 0 or after is not None, 'extra custody page follows the last selected proof')
            report, _ = observation_item(parent, entry_item, stages, 'custody', common, budget)
            selection = REVIEW['custody_page'](store, mint, mint_url, checkpoint, stage_sha, store_sha, after, total)
            matches_selection(report, selection, common)
            require(all(report[k] == selection[k] for k in (
                        'staged_receipt_sha256', 'mint_store_sha256', 'mint_id', 'total_candidates', 'after_y', 'next_after_y'))
                    and report['mint_url'].rstrip('/') == mint_url.rstrip('/'),
                    'custody observation is out of order or belongs to another mint')
            for key, value in custody_amounts(selection['proofs'], report).items():
                totals[key] = totals.get(key, 0) + value
            count += len(selection['proofs'])
            after = selection['next_after_y']
            observations.append(entry_item['sha256'])
            when = report['observed_at']
            first_at = min(first_at, when) if first_at is not None else when
            last_at = max(last_at, when) if last_at is not None else when
        require(after is None and count == total, 'custody observations omit a required page')
    return {'mint_id': mint, 'staged_receipt_sha256': stage_sha, 'mint_store_sha256': store_sha,
            'selected_count': count, 'observation_sha256': observations,
            'observed_from': first_at, 'observed_through': last_at, **totals}


def compare_custody_all(entry, parent, stages, wallet_source, common, budget):
    require(set(entry) == {'staged_receipt_sha256', 'mints'} and isinstance(entry['mints'], dict),
            'invalid complete custody evidence fields')
    stage_sha = entry['staged_receipt_sha256']
    wallet_path, wallet_files = wallet_source
    with REVIEW['custody_inventory'](wallet_path, wallet_files, common, budget) as inventory:
        require(set(entry['mints']) == set(inventory), 'custody evidence must cover every retained mint exactly once')
        reports = {}
        for mint, namespace in inventory.items():
            selection = {'staged_receipt_sha256': stage_sha, 'mint_id': mint, 'observations': entry['mints'][mint]}
            reports[mint] = compare_custody(selection, parent, stages,
                                            (wallet_path, namespace['mint_store_sha256']), common, budget)
    return {'staged_receipt_sha256': stage_sha, 'mint_namespaces_compared': len(reports), 'mints': reports,
            'observed_from': min((r['observed_from'] for r in reports.values()), default=None),
            'observed_through': max((r['observed_through'] for r in reports.values()), default=None),
            **{key: sum(r[key] for r in reports.values())
               for key in ('selected_count', *CUSTODY_TOTALS, 'mint_unspent_held_sats')}}


def original_ids(*groups):
    ids = set()
    for group in groups:
        require(group['next_after'] is None and group['total'] == len(group['items']),
                'recovery worklist exceeds the batch bound')
        for item in group['items']:
            ids.add(valid_id((item['staged'] or item['reference'])['id']))
    return ids


def exact_observation_entries(entries, field, expected):
    require(isinstance(entries, list) and len(entries) <= BATCH_LIMIT,
            'invalid bounded recovery observation batch')
    indexed = {valid_id(item[field]): item for item in entries}
    require(len(indexed) == len(entries) and set(indexed) == expected,
            'recovery batch must cover every required original operation exactly once')
    return [indexed[id] for id in sorted(indexed)]


def custody_evidence(plan, staged_sha, reference_sha):
    version = plan['version']
    require(type(version) is int and version in (1, 2, 3), 'invalid recovery evidence manifest version')
    required = {'version', 'treasury', 'feeder'}
    if version == 3:
        names = {'custody_all': staged_sha, 'reference_custody_all': reference_sha}
        require(reference_sha is not None and reference_sha != staged_sha and set(plan) == required | set(names),
                'paired custody requires both checkpoint evidence batches')
        for name, expected in names.items():
            entry = plan[name]
            require(isinstance(entry, dict) and entry.get('staged_receipt_sha256') == expected,
                    'custody batch is assigned to the wrong checkpoint')
        return {name: plan[name] for name in names}
    custody, custody_all = plan.get('custody'), plan.get('custody_all')
    require(set(plan) - {'custody', 'custody_all'} == required
            and (custody is None or (custody_all is None and version == 1))
            and (version == 2 or any(item is not None for item in (plan['treasury'], plan['feeder'], custody, custody_all))),
            'invalid recovery evidence manifest')
    return {name: plan[name] for name in ('custody', 'custody_all') if plan.get(name) is not None}


def compare(plan, parent, stages, staged_sha, reference_sha, common, budget, wallet_sources=None):
    custody_batches = custody_evidence(plan, staged_sha, reference_sha)
    version = plan['version']
    staged = stages[staged_sha]
    reference = stages[reference_sha] if reference_sha is not None else None
    # Reuse history and immutable-identity checks across all worklist groups.
    groups = REVIEW['worklist'](*staged[1:], *(reference[1:] if reference else (None, None)),
                                limit=BATCH_LIMIT if version >= 2 else 1, count_missing=version == 3)
    result = {'version': version, 'result': 'CASHU_RESTORE_OBSERVATIONS_COMPARED', 'checkpoint_id': staged[0],
            'staged_receipt_sha256': staged_sha, 'reference_checkpoint_id': reference[0] if reference else None,
            'reference_receipt_sha256': reference_sha,
            'worklist_counts': {key: value['total'] for key, value in groups.items()},
            'custody': None,
            **NO_AUTHORITY}
    comparison_args = (parent, stages, staged_sha, reference_sha, common, budget)
    if version == 1:
        result['treasury'] = (compare_treasury(plan['treasury'], *comparison_args)
                              if plan['treasury'] is not None else None)
        result['feeder'] = (compare_feeder(plan['feeder'], *comparison_args)
                            if plan['feeder'] is not None else None)
    else:
        treasury = exact_observation_entries(plan['treasury'], 'intent_id',
                                             original_ids(groups['strike_receives'], groups['mint_melts']))
        feeder = exact_observation_entries(plan['feeder'], 'request_id', original_ids(groups['feeder_requests']))
        result['treasury'] = [compare_treasury(entry, *comparison_args) for entry in treasury]
        result['feeder'] = [compare_feeder(entry, *comparison_args) for entry in feeder]
    for name, custody in custody_batches.items():
        wallet_source = wallet_sources[custody['staged_receipt_sha256']]
        if name == 'custody':
            wallet, files = wallet_source
            store = REVIEW['mint_store_name'](custody['mint_id'], common)
            result[name] = compare_custody(custody, parent, stages, (wallet, files[store]), common, budget)
        else:
            result[name] = compare_custody_all(custody, parent, stages, wallet_source, common, budget)
    if version == 3:
        pending = REVIEW['worklist'](*reference[1:], limit=1)
        result['reference_pending_counts'] = {key: value['total'] for key, value in pending.items()}
        result['reference_missing_counts'] = {key: value['missing_from_reference'] for key, value in groups.items()}
    return result


def compare_checkpoints(staged, staged_sha, evidence, evidence_sha, reference, reference_sha, common, budget):
    require(bool(reference) == bool(reference_sha), 'reference requires its reviewed receipt hash')
    selected = [(staged, staged_sha)]
    if reference is not None:
        selected.append((reference, reference_sha))
    document = read_bound(evidence, evidence_sha, 512 * 1024, common, budget)
    batches = custody_evidence(document, staged_sha, reference_sha)
    custody_receipts = {batch['staged_receipt_sha256'] for batch in batches.values()}
    require(custody_receipts <= {sha for _, sha in selected}, 'custody evidence requires its verified mint stage')
    stages, wallet_sources = {}, {}
    with ExitStack() as stack:
        for directory, sha in selected:
            require(sha not in stages, 'a comparison requires independently retained stages')
            if sha in custody_receipts:
                checkpoint, core, wallet, files = REVIEW['open_images'](
                    directory, sha, common, budget, wallet_inventory=True)
                wallet_sources[sha] = (wallet, files)
            else:
                checkpoint, core, wallet = REVIEW['open_images'](directory, sha, common, budget)
            require(all(other[0] != checkpoint for other in stages.values()), 'checkpoint cannot reference itself')
            stages[sha] = (checkpoint, *(stack.enter_context(closing(REVIEW['open_database'](file, budget)))
                                         for file in (core, wallet)))
        result = compare(document, evidence.parent, stages, staged_sha, reference_sha, common, budget, wallet_sources)
    result['evidence_sha256'] = evidence_sha
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('staged', type=Path)
    parser.add_argument('staged_sha256')
    parser.add_argument('evidence', type=Path)
    parser.add_argument('evidence_sha256')
    parser.add_argument('--reference', type=Path)
    parser.add_argument('--reference-sha256')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        verified_tool_identity()
        require(bool(args.reference) == bool(args.reference_sha256), 'reference requires its reviewed receipt hash')
        common = runpy.run_path(str(HERE / 'collect-cashu-backup.py'))
        common['protected_directory'](args.output.parent, private=True)
        require(not args.output.is_relative_to(args.staged)
                and (args.reference is None or not args.output.is_relative_to(args.reference)),
                'output must be outside the independently retained stages')
        budget = common['Budget'](0, 300)
        result = compare_checkpoints(args.staged, args.staged_sha256, args.evidence, args.evidence_sha256,
                                     args.reference, args.reference_sha256, common, budget)
        common['write_json'](args.output, result)
        common['sync'](args.output.parent)
        print(json.dumps({'result': result['result'], 'resume_authorized': False}, sort_keys=True))
    except (ValueError, OSError, KeyError, TypeError, IndexError, AttributeError, sqlite3.Error):
        parser.exit(1, 'Cashu restore reconciliation review refused. Preserve the fenced checkpoint.\n')


if __name__ == '__main__':
    if 'verified_tool_identity' not in globals():
        raise SystemExit('use the externally verified root-owned backup launcher')
    main()
