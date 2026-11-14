#!/usr/bin/env python3
"""Prepare four matching financial configurations; never install or activate.

Authenticated root-owned launcher only. Preserve original identities, credentials,
epoch, receive/contact/view/messaging configuration and running services. Require
explicit limits and the already selected stored member allocation. Native checking
uses the original core UID without credentials or networking. Backlog, provider and
operator activation review remain separate; this command opens no journal.
"""
import sys
sys.dont_write_bytecode = True
if 'verified_tool_identity' not in globals():
    if __name__ == '__main__':
        raise SystemExit('use the externally verified root-owned tool launcher')

    def verified_tool_identity():
        raise ValueError('verified tool launcher required')

import argparse
import copy
import importlib.util
import json
import os
from pathlib import Path
import re
import tempfile
import time
import tomllib
import urllib.request
from urllib.parse import urlsplit

spec = importlib.util.spec_from_file_location('financial_stage_support', Path(__file__).with_name('maintain-contact-upgrade.py'))
U = importlib.util.module_from_spec(spec)
spec.loader.exec_module(U)
U.verified_tool_identity = lambda: verified_tool_identity()
C, Q, S, TX = U.C, U.Q, U.S, U.TX
LIMITS = {'member_percent', 'max_sats_per_feed', 'max_sats_per_24h',
          'max_fee_sats', 'max_net_sats', 'max_new_per_hour'}
USERS = {'herd', 'dexter', 'rowan', 'cosmo', 'newton', 'nova'}


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


def validate_limits(limits):
    if (set(limits) not in (LIMITS, LIMITS | {'fee_mode'})
            or any(type(limits[n]) is not int or limits[n] < 0 for n in LIMITS)):
        raise ValueError('all six explicit integer financial limits and an optional fee mode required')
    mode = limits.get('fee_mode', 'fixed_allowance')
    if type(mode) is not str or mode not in {'fixed_allowance', 'deduct_from_share'}:
        raise ValueError('unknown financial fee mode')
    if mode == 'deduct_from_share' and limits['max_fee_sats'] != 0:
        raise ValueError('deduct-from-share has no independent fixed fee cap')
    if (limits['member_percent'] > 100 or limits['max_new_per_hour'] > 3600
            or limits['max_sats_per_24h'] > 2**63 - 1
            or limits['max_net_sats'] > (2**63 - 1) // 1000
            or limits['max_sats_per_feed'] > limits['max_sats_per_24h']
            or limits['max_fee_sats'] > limits['max_sats_per_feed']
            or limits['max_net_sats'] > limits['max_sats_per_feed']):
        raise ValueError('financial limits exceed their bounded scope')
    if any(limits[n] for n in LIMITS) and any(limits[name] == 0 for name in LIMITS - {'max_fee_sats'}):
        raise ValueError('use all-zero limits or a complete positive allocation and admission policy')


def render(original, identities, plans_gid, limits):
    validate_limits(limits)
    if set(original) != {'core', 'membership', 'payout', 'signer'} or set(identities) != set(original):
        raise ValueError('complete four-role original configuration and identities required')
    uids = [identities[role]['uid'] for role in original]
    if (any(type(uid) is not int or uid <= 0 for uid in uids) or len(set(uids)) != 4
            or type(plans_gid) is not int or plans_gid <= 0):
        raise ValueError('four distinct non-admin identities and existing plan group required')
    core, member, payout, signer = (original[name] for name in ('core', 'membership', 'payout', 'signer'))
    bridge = core['core_bridge']
    owner = member['project_pubkey']
    if (member.get('view', {}).get('enabled') is not True
            or member['view'].get('listen') != '127.0.0.1:3340'
            or bridge['peer_uid'] != identities['membership']['uid']
            or member['core']['core_uid'] != identities['core']['uid']
            or payout['intake_uid'] != identities['membership']['uid']
            or payout['signer_uid'] != identities['signer']['uid']
            or signer['policy']['peer_uid'] != identities['payout']['uid']
            or payout['policy']['scope']['core_uid'] != identities['core']['uid']
            or payout['policy']['scope']['owner'] != owner or signer['policy']['owner'] != owner
            or core['nostr']['bunker_pubkey'] != owner or signer['nostr']['bunker_pubkey'] != owner
            or member['core']['epoch'] != payout['policy']['scope']['epoch']
            or payout['core_socket'] != member['core_socket'] or bridge['socket'] != member['core_socket']
            or payout['signer_socket'] != signer['socket']
            or payout['policy']['receipt_relays'] != signer['policy']['relays']
            or core['lnurl']['zap']['receipt_pubkey'] != owner
            or core['lnurl']['zap']['payee_pubkeys'] != member['core']['payees']
            or bridge['reward_policy']['owner'] != owner
            or set(member['core']['payees']) != USERS
            or {row['user'] for row in core['lightning_address']} != USERS):
        raise ValueError('original financial identities, epoch, payees, sockets or receipt relays differ')
    operations = set(payout['policy']['operation_addresses'])
    for origin in [core['lnurl']['public_base_url'], *core['lnurl']['zap'].get('alias_origins', [])]:
        url = urlsplit(origin)
        if url.scheme != 'https' or not url.hostname or url.username or url.password or url.port not in (None, 443):
            raise ValueError('existing canonical HTTPS operation-address origins required')
        operations.update(user + '@' + url.hostname for user in USERS)
    if len(operations) > 128:
        raise ValueError('complete operation-address exclusions exceed policy bound')
    candidate = copy.deepcopy(original)
    core, member, payout, signer = (candidate[name] for name in ('core', 'membership', 'payout', 'signer'))
    reward = core['core_bridge']['reward_policy']
    reward.update(member_percent_ceiling=limits['member_percent'],
                  max_sats_per_feed=limits['max_sats_per_feed'], max_sats_per_24h=limits['max_sats_per_24h'])
    core['core_bridge'].update(payout_peer_uid=identities['payout']['uid'],
        payout_account_id=payout['policy']['scope']['provider']['account_id'], payout_execution_enabled=True)
    # This aligns the seed/configuration with the separately observed stored
    # setting. It is never a command to modify the running membership journal.
    member['settings']['member_percent'] = limits['member_percent']
    # Operator decision 2026-11-11: three seats. As with the allocation seed,
    # this must match separately selected stored state, not reset an owner.
    member['settings']['max_members'] = 3
    member['rewards'] = {'fee_basis_points': 0, 'minimum_fee_sats': limits['max_fee_sats'],
        'minimum_net_sats': 1, 'maximum_net_sats': limits['max_net_sats'],
        'profile_max_age_seconds': 3600, 'operation_addresses': sorted(operations)}
    member['payout_bridge'] = {'socket': payout['intake_socket'], 'peer_uid': identities['payout']['uid'],
                              'group_gid': plans_gid}
    payout['enabled'] = True
    payout['policy'].update(execution_enabled=True, member_percent_ceiling=limits['member_percent'],
        max_sats_per_feed=limits['max_sats_per_feed'], max_sats_per_24h=limits['max_sats_per_24h'],
        max_fee_sats=limits['max_fee_sats'], max_net_sats=limits['max_net_sats'],
        operation_addresses=sorted(operations))
    mode = limits.get('fee_mode', 'fixed_allowance')
    if mode == 'deduct_from_share':
        member['rewards']['fee_mode'] = mode
        payout['policy']['fee_mode'] = mode
    else:
        # Preserve the historical candidate representation and do not inherit a
        # different mode from a previously disabled configuration.
        payout['policy'].pop('fee_mode', None)
    signer['enabled'] = True
    signer['policy'].update(max_net_sats=limits['max_net_sats'], max_new_per_hour=limits['max_new_per_hour'])
    return candidate


def observed_allocation(owner, percent):
    request = urllib.request.Request('http://127.0.0.1:3340/api/v1/cyberherd', headers={'Accept': 'application/json'})
    with urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect()).open(request, timeout=3) as response:
        raw = response.read(98305)
        if (response.status != 200 or len(raw) > 98304
                or response.headers.get('Content-Type', '').split(';')[0].strip().lower() != 'application/json'):
            raise ValueError('bounded existing native roster snapshot required')
    value = json.loads(raw)
    roster = value['roster']
    if (value.get('version') != 1 or type(value.get('generated_at')) is not int
            or abs(time.time() - value['generated_at']) > 30
            or roster.get('owner') != owner or type(roster.get('member_percent')) is not int
            or roster['member_percent'] != percent or type(roster.get('capacity')) is not int
            or roster['capacity'] != 3):
        raise ValueError('select the requested stored allocation and three-user capacity through the operator interface before preparation')
    return {'owner': owner, 'member_percent': percent, 'capacity': 3, 'generated_at': value['generated_at']}


def check_report(raw):
    value = json.loads(raw)
    expected = {'accepted': True, 'configuration_only': True, 'candidate_limits_valid': True,
                'journals_opened': False, 'credentials_loaded': False, 'provider_operations': 0,
                'services_changed': False}
    if (set(value) != set(expected)
            or any(type(value[k]) is not type(v) or value[k] != v for k, v in expected.items())):
        raise ValueError('native financial configuration checker did not positively accept the candidate')
    return value


def native_validation(directory, baseline, validator, validator_fp):
    if TX.fingerprint(validator) != validator_fp:
        raise ValueError('source-pinned financial checker changed')
    component = baseline['identities']['core']
    # The checker must be executable by the original non-root UID. Runtime
    # mounts may refuse executing /run files; keep the same protected/private
    # carrier contract on disk rather than weakening a mount or unit sandbox.
    C.protected(Path('/var/lib'), directory=True)
    carrier = Path(tempfile.mkdtemp(prefix='lg-financial-check-', dir='/var/lib'))
    carrier.chmod(0o711)
    try:
        program = carrier / 'checker'
        TX.copy_durable(validator, program, validator_fp)
        paths = []
        for role in ('core', 'membership', 'payout', 'signer'):
            path = carrier / (role + '.toml')
            Q.I.publish(path, (directory / ('candidate-' + role + '.toml')).read_bytes(), 0o440)
            os.chown(path, 0, component['gid'])
            paths.append(str(path))
        result = C.command('/usr/bin/unshare', '--net', '--', '/usr/bin/setpriv',
            '--reuid=' + str(component['uid']), '--regid=' + str(component['gid']), '--clear-groups',
            '--inh-caps=-all', '--ambient-caps=-all', '--bounding-set=-all', '--no-new-privs',
            str(program), *paths, timeout=30)
        accepted = check_report(result)
    finally:
        carrier.chmod(0o700)
        TX.sync_directory(carrier)
    if TX.fingerprint(validator) != validator_fp:
        raise ValueError('source-pinned financial checker changed during validation')
    return accepted


def prepare(directory, validator, validator_sha, limits):
    C.root()
    validate_limits(limits)
    C.protected(C.BASE, directory=True, private=True)
    if directory.parent != C.BASE or re.fullmatch('[A-Za-z0-9_.-]{1,128}', directory.name) is None:
        raise ValueError('new direct private upgrade-base child required')
    if directory.exists() or directory.is_symlink():
        raise FileExistsError('financial preparation destination must be unused')
    baseline = U.P.baseline(current_schema=True)
    C.protected(validator)
    validator_fp = TX.fingerprint(validator)
    if (not re.fullmatch('[0-9a-f]{64}', validator_sha) or validator_fp['sha256'] != validator_sha
            or validator_fp['mode'] != 0o755 or validator.stat().st_size > 64 * 1024 * 1024):
        raise ValueError('independently authenticated native financial checker required')
    configs = {'core': C.CONFIG, **{role: Q.ETC / name / 'config.toml'
        for role, name in U.P.ROLE_NAMES.items() if role != 'core'}}
    original = {role: tomllib.loads(path.read_text()) for role, path in configs.items()}
    plans_gid = C.grp.getgrnam('cyberherd-plans').gr_gid
    candidate = render(original, baseline['identities'], plans_gid, limits)
    projection = observed_allocation(original['membership']['project_pubkey'], limits['member_percent'])
    directory.mkdir(mode=0o700)
    TX.sync_directory(directory.parent)
    C.write_json(directory / 'INCOMPLETE', {'services_changed': False})
    rows = []
    for role, path in configs.items():
        TX.copy_durable(path, directory / ('original-' + role + '.toml'), baseline['files'][str(path)])
        staged = directory / ('candidate-' + role + '.toml')
        Q.I.publish(staged, S.W.toml_text(candidate[role]).encode(), 0o600)
        if tomllib.loads(staged.read_text()) != candidate[role]:
            raise ValueError('native candidate serialization changed configuration')
        rows.append({'destination': str(path), 'original': baseline['files'][str(path)],
                     'candidate_sha256': U.digest(staged)})
    checker = directory / 'native-checker'
    TX.copy_durable(validator, checker, validator_fp)
    accepted = native_validation(directory, baseline, checker, validator_fp)
    for role, row in zip(configs, rows):
        if (U.digest(directory / ('candidate-' + role + '.toml')) != row['candidate_sha256']
                or TX.fingerprint(directory / ('original-' + role + '.toml')) != row['original']):
            raise ValueError('reviewed candidate or retained original changed during validation')
    if (U.P.baseline(current_schema=True) != baseline
            or C.grp.getgrnam('cyberherd-plans').gr_gid != plans_gid):
        raise ValueError('original installation or process changed during financial preparation')
    observed_allocation(original['membership']['project_pubkey'], limits['member_percent'])
    C.write_json(directory / 'PREPARED.json', {'version': 1, 'tool': verified_tool_identity(),
        'financial_candidate_only': True, 'activation_authorized': False,
        'baseline': baseline, 'limits': limits, 'plans_group_gid': plans_gid, 'configurations': rows,
        'observed_member_allocation': projection, 'native_checker': validator_fp,
        'native_validation': accepted, 'backlog_review_required': True,
        'provider_qualification_required': True, 'services_changed': False, 'journals_opened': False})
    (directory / 'INCOMPLETE').unlink()
    TX.sync_directory(directory)
    return {'prepared': True, 'financial_candidate_only': True, 'activation_authorized': False,
            'services_changed': False, 'journals_opened': False, 'backlog_review_required': True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--validator', required=True, type=Path)
    parser.add_argument('--validator-sha256', required=True)
    for name in sorted(LIMITS):
        parser.add_argument('--' + name.replace('_', '-'), required=True, type=int)
    parser.add_argument('--fee-mode', choices=['fixed_allowance', 'deduct_from_share'])
    args = parser.parse_args()
    limits = {name: getattr(args, name) for name in LIMITS}
    if args.fee_mode is not None:
        limits['fee_mode'] = args.fee_mode
    print(json.dumps(prepare(args.directory, args.validator, args.validator_sha256, limits), sort_keys=True))


if __name__ == '__main__':
    main()
