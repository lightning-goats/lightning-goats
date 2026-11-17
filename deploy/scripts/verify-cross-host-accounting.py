#!/usr/bin/env python3
"""Check captured synthetic accounting against the pinned HOME delivery journal.

Offline evidence only: never opens a network connection, starts a service, sends
commands, changes SQLite, or establishes provenance of operator-supplied captures.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sqlite3
import stat
import uuid

OWNER_SHA = '1cacb11569ab90db03bc0ee2f94fd3fec9d0e2132bc816d4f5982d4fd7d6a88c'
MAX_INPUT = 262144


def require(condition, message):
    if not condition:
        raise ValueError(message)


def canonical_id(value):
    require(isinstance(value, str) and str(uuid.UUID(value)) == value, 'noncanonical UUID')
    return value


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'duplicate JSON field')
        result[key] = value
    return result


def read_json(path):
    require(stat.S_ISREG(path.lstat().st_mode), 'input must be a regular non-symlink file')
    with path.open('rb') as stream:
        raw = stream.read(MAX_INPUT + 1)
    require(len(raw) <= MAX_INPUT, 'input exceeds capture bound')
    return json.loads(raw, object_pairs_hook=unique_object), hashlib.sha256(raw).hexdigest()


def journal(snapshot):
    require(snapshot['source_sha256'] == OWNER_SHA, 'unreviewed HOME fixture source')
    rows = snapshot['deliveries']
    require(isinstance(rows, list) and len(rows) <= 128, 'journal capacity exceeded')
    require(type(snapshot['count']) is int and snapshot['count'] == len(rows), 'delivery count/journal gap')
    require(snapshot['hold'] == 'ON' and snapshot['remote_enabled'] == 'OFF', 'capture requires hold ON and remote OFF')
    # The HOME control helper refuses Fault ON before producing this snapshot.
    for sequence, row in enumerate(rows, 1):
        require(type(row['sequence']) is int and row['sequence'] == sequence, 'journal sequence gap')
        canonical_id(row['requestId'])
        require(row['status'] == 'released', 'unresolved or invalid HOME delivery')
        require(isinstance(row.get('receivedAt'), str) and isinstance(row.get('releasedAt'), str), 'missing delivery timestamps')
    return rows


def rows(connection, statement):
    result = connection.execute(statement).fetchmany(129)
    require(len(result) <= 128, 'unexpected synthetic database size')
    return [dict(row) for row in result]


def verify(database, baseline, completed, run_id):
    canonical_id(run_id)
    old = journal(baseline)
    new = journal(completed)
    require(new[:len(old)] == old, 'baseline journal changed or rolled back')
    delivered = new[len(old):]
    require(len(delivered) == 2, 'expected exactly two delivered owner commands')
    ids = [row['requestId'] for row in delivered]
    require(len(set(ids)) == 2, 'repeated UUID delivered more than once')
    require(not set(ids).intersection(row['requestId'] for row in old), 'old UUID redelivered')
    require(completed['ack'] == ids[-1], 'final acknowledgement does not match final delivery')
    require(stat.S_ISREG(database.lstat().st_mode), 'database must be regular and non-symlink')
    # Use a quiesced exported database with its consistent WAL when present.
    # mode=ro prevents accidental initialization/migration or financial writes.
    connection = sqlite3.connect(database.resolve().as_uri() + '?mode=ro', uri=True, timeout=2)
    connection.row_factory = sqlite3.Row
    connection.setlimit(sqlite3.SQLITE_LIMIT_LENGTH, MAX_INPUT)
    connection.setlimit(sqlite3.SQLITE_LIMIT_ATTACHED, 0)
    budget = [10000]
    def limit():
        budget[0] -= 1
        return budget[0] <= 0
    connection.set_progress_handler(limit, 1000)
    try:
        connection.execute('PRAGMA query_only=ON')
        connection.execute('BEGIN')
        payments = rows(connection, 'SELECT source,source_id,address_user,credit_pool,amount_msat FROM settled_payments')
        require(payments == [{'source':'synthetic-cross-host','source_id':run_id,'address_user':'herd','credit_pool':'herd','amount_msat':2340000}], 'unexpected synthetic settlement')
        entries = rows(connection, 'SELECT entry_type,source_key,delta_sats,payment_source,payment_source_id,feed_attempt_id FROM ledger_entries ORDER BY id')
        receipts = [row for row in entries if row['entry_type'] == 'HERD_RECEIPT']
        require(len(receipts) == 1 and receipts[0]['delta_sats'] == 2340 and receipts[0]['payment_source'] == 'synthetic-cross-host' and receipts[0]['payment_source_id'] == run_id, 'synthetic credit mismatch')
        debits = [row for row in entries if row['entry_type'] == 'FEED_DEBIT']
        require(len(entries) == 3 and len(debits) == 2, 'unexpected ledger entries')
        require([row['feed_attempt_id'] for row in debits] == ids and all(row['delta_sats'] == -1000 and row['source_key'] == 'feed:' + row['feed_attempt_id'] for row in debits), 'debits do not match delivered UUIDs')
        require(sum(row['delta_sats'] for row in entries) == 340, 'remaining balance mismatch')
        attempts = rows(connection, 'SELECT id,status,threshold_sats FROM feed_attempts')
        confirmed = [row for row in attempts if row['status'] == 'confirmed']
        require({row['id'] for row in confirmed} == set(ids) and len(confirmed) == 2, 'confirmation identity mismatch')
        require(all(row['threshold_sats'] == 1000 and row['status'] in ('confirmed','reconciled_not_fed') for row in attempts), 'unresolved or unexpected feed attempt')
        require(not set(row['id'] for row in attempts if row['status'] == 'reconciled_not_fed').intersection(ids), 'refused UUID was delivered')
        events = rows(connection, 'SELECT event_type,substr(payload_json,1,65537) AS payload_json FROM event_log ORDER BY seq')
        feed_events = [json.loads(row['payload_json'], object_pairs_hook=unique_object) for row in events if row['event_type'] == 'feeder_confirmed']
        require([row['feed_attempt_id'] for row in feed_events] == ids, 'confirmation events do not match deliveries')
        require([row['feed_credit_sats'] for row in feed_events] == [1340,340] and all(row['threshold_sats'] == 1000 for row in feed_events), 'confirmation event accounting mismatch')
        payment_events = [json.loads(row['payload_json'], object_pairs_hook=unique_object) for row in events if row['event_type'] == 'payment_received']
        require(len(payment_events) == 1, 'duplicate or missing payment event')
        payment_event = payment_events[0]
        require(all(payment_event.get(key) == value for key, value in {'source':'synthetic-cross-host','source_id':run_id,'address_user':'herd','credit_pool':'herd','amount_sats':2340,'feed_credit_sats':2340}.items()), 'payment event does not match seed')
        require(connection.execute('SELECT count(*) FROM message_outbox').fetchone()[0] == 0, 'synthetic run contains public Nostr outbox work')
        require(connection.execute('SELECT count(*) FROM strike_receive_requests').fetchone()[0] == 0, 'synthetic run issued provider requests')
    finally:
        connection.close()
    return {'scope':'offline captured synthetic accounting comparison; not live acceptance', 'run_id':run_id,
            'owner_source_sha256':OWNER_SHA,'baseline_commands':len(old),'delivered_commands':2,
            'confirmed_request_ids':ids,'confirmed_debits':2,'remaining_sats':340,
            'not_proven':['capture provenance/freshness','network identity and containment','scenario execution and service sandbox','physical feeding or real provider settlement']}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--database', type=Path, required=True)
    parser.add_argument('--baseline', type=Path, required=True)
    parser.add_argument('--completed', type=Path, required=True)
    parser.add_argument('--run-id', required=True)
    args = parser.parse_args()
    before, before_hash = read_json(args.baseline)
    after, after_hash = read_json(args.completed)
    report = verify(args.database, before, after, args.run_id)
    report['home_capture_sha256'] = {'baseline':before_hash,'completed':after_hash}
    print(json.dumps(report, indent=2, sort_keys=True))
