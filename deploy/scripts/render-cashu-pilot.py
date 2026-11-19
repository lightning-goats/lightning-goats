#!/usr/bin/env python3
"""Render a private, inactive Minibits pilot configuration for offline review."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import tomllib
from urllib.parse import urlsplit
import uuid


MINT = 'https://mint.minibits.cash/Bitcoin'
MINT_ID = 'minibits'
POLICY_VERSION = 'minibits-pilot-v1'
CUSTODY_SATS = 999
FEE_SATS = 5
ROLLING_FEE_SATS = 25
HOLDING_SECONDS = 1800
INTAKE = '/run/lightning-goats-cashu/intake.sock'
RECEIPTS = '/run/lightning-goats-cashu-receipts/receipt.sock'
TREASURY = '/run/lightning-goats-cashu/treasury.sock'
SIGNER = '/run/lightning-goats-cashu-signer/sign.sock'
EPOCHS = ('core_epoch', 'wallet_instance', 'wallet_epoch', 'broker_instance', 'broker_epoch')
RUNTIME_IDS = ('core_uid', 'wallet_uid', 'wallet_gid', 'signer_uid', 'signer_gid')
REQUIRED = {*EPOCHS, *RUNTIME_IDS, 'version', 'receiving_key', 'client_pubkey', 'helper_sha256',
            'strike_account_id', 'strike_credential_fingerprint', 'daily_credit_sats',
            'minimum_deposit_sats', 'mint_keysets', 'relay_since_unix'}


def exact_id(value):
    return isinstance(value, str) and uuid.UUID(value).int > 0 and str(uuid.UUID(value)) == value


def hex_value(value, pattern='[0-9a-f]{64}'):
    return isinstance(value, str) and re.fullmatch(pattern, value) is not None and any(c != '0' for c in value)


def number(value, minimum, maximum):
    return type(value) is int and minimum <= value <= maximum


def relay_set(values):
    if not isinstance(values, list) or not 1 <= len(values) <= 8 or len(set(values)) != len(values):
        raise ValueError('a bounded, distinct relay list is required')
    for value in values:
        if not isinstance(value, str) or any(c.isspace() or c in '\\"' for c in value):
            raise ValueError('invalid configured relay')
        parts = urlsplit(value)
        if (parts.scheme != 'wss' or not parts.hostname or parts.port not in (None, 443)
                or parts.username is not None or '?' in value or '#' in value):
            raise ValueError('approved relay must be WSS without login or query')
    return values


def receive_relay_subset(selection, approved):
    def identity(value):
        parts = urlsplit(value)
        return parts.hostname, parts.path or '/'

    selected = relay_set(selection)
    identities = {identity(relay) for relay in selected}
    if len(identities) != len(selected) or not identities.issubset(map(identity, approved)):
        raise ValueError('choose distinct Nutzap relays from the existing project list')
    return selected


def validate(inputs):
    if (not isinstance(inputs, dict) or set(inputs) - {'nutzap_relays'} != REQUIRED
            or type(inputs['version']) is not int or inputs['version'] != 1):
        raise ValueError('complete version-1 pilot bindings required')
    if not all(exact_id(inputs[key]) for key in (*EPOCHS, 'strike_account_id')) or len({inputs[key] for key in EPOCHS}) != len(EPOCHS):
        raise ValueError('distinct canonical registration UUIDs required')
    if (not all(number(inputs[key], 1, 2**32 - 2) for key in RUNTIME_IDS)
            or len({inputs[key] for key in ('core_uid', 'wallet_uid', 'signer_uid')}) != 3
            or inputs['wallet_gid'] == inputs['signer_gid']):
        raise ValueError('isolated core, wallet and signer identities required')
    if (not hex_value(inputs['receiving_key'], '0[23][0-9a-f]{64}')
            or not all(hex_value(inputs[key]) for key in ('client_pubkey', 'helper_sha256', 'strike_credential_fingerprint'))):
        raise ValueError('reviewed receiving key, client public key and exact digests required')
    if not number(inputs['daily_credit_sats'], CUSTODY_SATS, 2**63 - 1):
        raise ValueError('rolling credit budget must cover the largest permitted receipt')
    if (not number(inputs['minimum_deposit_sats'], 1, CUSTODY_SATS - FEE_SATS)
            or not number(inputs['relay_since_unix'], 1, 2**63 - 31)):
        raise ValueError('explicit minimum deposit and relay start required')
    keysets = inputs['mint_keysets']
    if not isinstance(keysets, list) or not 1 <= len(keysets) <= 16:
        raise ValueError('reviewed Minibits sat keysets required')
    ids = set()
    for keyset in keysets:
        if (not isinstance(keyset, dict) or set(keyset) != {'id', 'keys'}
                or not hex_value(keyset['id'], '(00[0-9a-f]{14}|01[0-9a-f]{64})')
                or keyset['id'] in ids or not isinstance(keyset['keys'], dict)
                or not 1 <= len(keyset['keys']) <= 64):
            raise ValueError('invalid pinned Minibits keyset')
        ids.add(keyset['id'])
        for amount, key in keyset['keys'].items():
            if (not isinstance(amount, str) or not re.fullmatch('[1-9][0-9]{0,18}', amount)
                    or int(amount) >= 2**63 or not hex_value(key, '0[23][0-9a-f]{64}')):
                raise ValueError('invalid pinned Minibits denomination')


def append_core(base, groups, approved):
    parts = [base, b'\n\n']
    for section, fields in groups.items():
        parts.append((f'[{section}]\n' + ''.join(
            f'{name} = {json.dumps(value)}\n' for name, value in fields.items()) + '\n').encode())
    parts.append(('[[cashu_receipts.approved]]\n' + ''.join(
        f'{name} = {json.dumps(value)}\n' for name, value in approved.items())).encode())
    return b''.join(parts)


def descriptor_selection(endpoint, preview):
    fields = {'version', 'id', 'request', 'encoding', 'status', 'service_started', 'descriptor_registered'}
    if (not isinstance(preview, dict) or set(preview) != fields
            or type(preview['version']) is not int or preview['version'] != 1
            or preview['status'] != 'prepared' or preview['service_started'] is not False
            or preview['descriptor_registered'] is not False
            or not isinstance(preview['id'], str)
            or re.fullmatch(r'lg1-[0-9a-f]{32}', preview['id']) is None):
        raise ValueError('expected the offline first-wallet request preview')
    encoding, request = preview['encoding'], preview['request']
    if encoding not in ('creqA', 'creqB') or not isinstance(request, str) or len(request) > 2048:
        raise ValueError('invalid request encoding or length')
    pattern = r'creqA[A-Za-z0-9_-]+={0,2}' if encoding == 'creqA' else r'CREQB1[023456789ACDEFGHJKLMNPQRSTUVWXYZ]+'
    if re.fullmatch(pattern, request) is None:
        raise ValueError('invalid request payload')
    parts = urlsplit(endpoint)
    if (len(endpoint.encode()) > 256 or any(c.isspace() or c in '\\' for c in endpoint)
            or parts.scheme != 'https' or not parts.hostname or parts.username is not None
            or parts.query or parts.fragment):
        raise ValueError('invalid public HTTPS request endpoint')
    return {'endpoint': endpoint, 'encoding': encoding}


def render(base, inputs, reviewed_request=None):
    validate(inputs)
    original = tomllib.loads(base.decode())
    if set(original) & {'cashu', 'cashu_receipts', 'cashu_treasury'}:
        raise ValueError('initial core configuration already contains Cashu settings')
    owner = original['nostr']['bunker_pubkey']
    relays = relay_set(original['nostr']['relays'])
    bunker = relay_set(original['nostr'].get('bunker_relays', relays))
    receive_relays = receive_relay_subset(inputs['nutzap_relays'], relays) if 'nutzap_relays' in inputs else relays
    receiver, client = inputs['receiving_key'], inputs['client_pubkey']
    if not hex_value(owner) or len({owner, receiver[2:], client}) != 3:
        raise ValueError('project, receiving and client public keys must be distinct')
    origin = original['strike']['api_url']
    parts = urlsplit(origin)
    if (parts.scheme != 'https' or not parts.hostname or parts.path not in ('', '/')
            or parts.username is not None or parts.query or parts.fragment):
        raise ValueError('existing Strike configuration requires a root HTTPS API origin')
    destination = dict(origin=origin.rstrip('/') + '/', account_id=inputs['strike_account_id'],
                       credential_fingerprint=inputs['strike_credential_fingerprint'], network='bitcoin')
    limits = dict(maximum_input_sats=CUSTODY_SATS, maximum_in_flight_sats=CUSTODY_SATS, maximum_fee_sats=FEE_SATS)
    mint_limits = dict(limits, minimum_deposit_sats=inputs['minimum_deposit_sats'], maximum_deposit_sats=CUSTODY_SATS)
    rolling = dict(unlimited_operations_per_24h=True, unlimited_input_sats_per_24h=True, fee_sats_per_24h=ROLLING_FEE_SATS)
    treasury = dict(mint_limits, **rolling)
    unit = dict(wallet_uid=inputs['wallet_uid'], wallet_instance=inputs['wallet_instance'],
                wallet_epoch=inputs['wallet_epoch'])
    wallet_binding = dict(wallet_instance=inputs['wallet_instance'], wallet_epoch=inputs['wallet_epoch'])
    wallet = dict(version=1, **wallet_binding, runtime_uid=inputs['wallet_uid'], core_uid=inputs['core_uid'],
                  ipc_gid=inputs['wallet_gid'], state_directory='/var/lib/lightning-goats-cashu',
                  intake_socket=INTAKE, receiving_key=receiver, recipient=owner, descriptors=[],
                  receive_cashu=False, receive_nutzap=False, body_bytes=65536, proof_count=64,
                  receipt_sats=CUSTODY_SATS, deadline_seconds=5, connections_per_second=8,
                  custody_limits=dict(maximum_total_sats=CUSTODY_SATS, maximum_per_mint_sats={MINT_ID: CUSTODY_SATS}),
                  mints=[dict(id=MINT_ID, advertised_url=MINT, policy_version=POLICY_VERSION,
                              receive_cashu=True, receive_nutzap=True, bolt11_exit_qualified=True)],
                  keysets=[dict(keyset, mint_id=MINT_ID) for keyset in inputs['mint_keysets']])
    if reviewed_request is not None:
        endpoint, preview = reviewed_request
        wallet['public_request'] = descriptor_selection(endpoint, preview)
        wallet['descriptors'] = [preview['id']]
    wallet['receipt_delivery'] = dict(enabled=False, socket=RECEIPTS, core_epoch=inputs['core_epoch'],
                                      maximum_attempts=8, deadline_seconds=5)
    wallet['relay_intake'] = dict(enabled=False, relays=receive_relays, historical_floor=inputs['relay_since_unix'],
                                  window_seconds=3600, overlap_seconds=120, event_limit=64)
    wallet['publication_signing'] = dict(socket=SIGNER, signer_uid=inputs['signer_uid'], announcements=False,
                                          redemptions=False, maximum_attempts=8)
    if 'nutzap_relays' in inputs:
        wallet['publication_delivery'] = dict(announcements=False, redemptions=False,
                                              base_relays=receive_relays, discovery_relays=receive_relays,
                                              maximum_attempts=8)
    wallet['treasury_intake'] = dict(enabled=False, accept_new=False, socket=TREASURY,
                                      core_epoch=inputs['core_epoch'], destination=destination,
                                      policies={MINT_ID: POLICY_VERSION})
    wallet['treasury_execution'] = dict(limits, **rolling, execute=False, reconcile=False, maximum_attempts=8)
    signer = dict(version=1, runtime_uid=inputs['signer_uid'], ipc_gid=inputs['signer_gid'],
                  state_directory='/var/lib/lightning-goats-cashu-signer', socket=SIGNER,
                  policy=dict(unit, broker_instance=inputs['broker_instance'], broker_epoch=inputs['broker_epoch'],
                              owner=owner, receiving_key=receiver[2:], mints=[MINT], relays=receive_relays),
                  nak=dict(binary='/usr/local/bin/nak-cashu-stdin', binary_sha256=inputs['helper_sha256'],
                           private_directory='/var/lib/lightning-goats-cashu-nak', owner=owner,
                           client_pubkey=client, relays=bunker, deadline_seconds=30))
    groups = {
        'cashu': dict(unit, socket=INTAKE, receive_cashu=False, receive_nutzap=False,
                      body_bytes=65536, concurrent_requests=4, deadline_seconds=5),
        'cashu_receipts': dict(unit, socket=RECEIPTS, ipc_gid=inputs['wallet_gid'], core_epoch=inputs['core_epoch'],
                               import_enabled=False, unlimited_receipts_per_24h=True,
                               credit_sats_per_24h=inputs['daily_credit_sats'], maximum_custody_sats=CUSTODY_SATS,
                               deadline_seconds=5, connections_per_second=8),
        'cashu_treasury': dict(socket=TREASURY, deadline_seconds=30),
        'cashu_treasury.controls': dict(plan=False, issue=False, deliver=False, inspect=False, maximum_attempts=8),
        'cashu_treasury.controls.sweep': dict(enabled=False, maximum_holding_age_seconds=HOLDING_SECONDS),
        'cashu_treasury.policy': dict(enabled=False, version=POLICY_VERSION, invoice_expiry_seconds=300),
        'cashu_treasury.policy.destination': destination,
        'cashu_treasury.policy.limits': treasury,
        'cashu_treasury.policy.approved_mints.minibits': mint_limits,
    }
    core = append_core(base, groups, dict(mint_id=MINT_ID, policy_version=POLICY_VERSION,
                                         maximum_gross_sats=CUSTODY_SATS, maximum_net_sats=CUSTODY_SATS))
    parsed = tomllib.loads(core.decode())
    if {name: parsed[name] for name in original} != original:
        raise ValueError('existing core configuration changed in the rendered copy')
    result = {'core.toml': core}
    result.update({name: (json.dumps(value, sort_keys=True, indent=2) + '\n').encode()
                   for name, value in (('wallet.json', wallet), ('signer.json', signer))})
    if any(len(result[name]) > 65536 for name in ('wallet.json', 'signer.json')):
        raise ValueError('rendered runtime configuration exceeds its native bound')
    return result


def read(path, maximum):
    with path.open('rb') as source:
        value = source.read(maximum + 1)
    if not 0 < len(value) <= maximum:
        raise ValueError('pilot input is empty or exceeds its size bound')
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('core_config', type=Path)
    parser.add_argument('bindings', type=Path)
    parser.add_argument('destination', type=Path)
    parser.add_argument('--reviewed-request', nargs=2, metavar=('HTTPS_ENDPOINT', 'PREVIEW_JSON'),
                        help='select an offline request preview while keeping receiving disabled')
    args = parser.parse_args()
    try:
        base = read(args.core_config, 131072)
        bindings = read(args.bindings, 65536)
        reviewed_request = None
        if args.reviewed_request is not None:
            endpoint, path = args.reviewed_request
            preview = read(Path(path), 4096)
            reviewed_request = (endpoint, json.loads(preview))
        files = render(base, json.loads(bindings), reviewed_request)
        receipt = dict(version=1, base_sha256=hashlib.sha256(base).hexdigest(),
                       bindings_sha256=hashlib.sha256(bindings).hexdigest(), activation_authorized=False,
                       files={name: hashlib.sha256(content).hexdigest() for name, content in files.items()})
        if reviewed_request is not None:
            receipt['request_preview_sha256'] = hashlib.sha256(preview).hexdigest()
        files['REVIEW.json'] = (json.dumps(receipt, sort_keys=True, indent=2) + '\n').encode()
        args.destination.mkdir(mode=0o700)
        for name, content in files.items():
            fd = os.open(args.destination / name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
            with os.fdopen(fd, 'wb') as output:
                output.write(content)
                output.flush()
                os.fsync(output.fileno())
        folder = os.open(args.destination, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            os.fsync(folder)
        finally:
            os.close(folder)
        print('Inactive Cashu configuration rendered for review.')
    except (ValueError, KeyError, TypeError, OSError):
        parser.exit(1, 'Cashu pilot rendering refused. Check the existing configuration and reviewed bindings.\n')


if __name__ == '__main__':
    main()
