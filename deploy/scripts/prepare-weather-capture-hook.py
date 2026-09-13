#!/usr/bin/env python3
"""Prepare an exclusive review patch for the pinned RTL producer; never applies it.

The optional observer runs only after JSON decode and preserves the original
station dispatch/HTTP behavior. Installation/restart needs separate host approval.
"""
import argparse
import difflib
import hashlib
import json
from pathlib import Path

SOURCE_SHA256 = '9f034f9d0ed5912ad5495136bcb6414c20bccb3cb24d1c09622a16c29c9e2777'


def patched_source(source):
    if hashlib.sha256(source.encode()).hexdigest() != SOURCE_SHA256:
        raise ValueError('deployed RTL source drift; inspect and re-pin before preparing')
    anchor = 'from collections import defaultdict\n'
    hook = '''
# Optional project observer: failure never interrupts household station ingestion.
try:
    import importlib.util as _lg_import
    _lg_spec = _lg_import.spec_from_file_location('lightning_goats_weather', '/usr/local/lib/lightning-goats-weather/lightning_goats_weather.py')
    _lg_module = _lg_import.module_from_spec(_lg_spec)
    _lg_spec.loader.exec_module(_lg_module)
    _lg_record_packet = _lg_module.record_packet
except Exception:
    _lg_record_packet = None
'''
    event = '                    data = json.loads(line)\n'
    capture = '''                    if _lg_record_packet is not None and isinstance(data, dict):
                        try:
                            _lg_record_packet('/var/lib/lightning-goats-weather/snapshot.db', data, WH65B_STATION_ID)
                        except Exception:
                            logging.warning('Lightning Goats weather observation unavailable')
'''
    if source.count(anchor) != 1 or source.count(event) != 1:
        raise ValueError('unexpected radio decoder structure')
    return source.replace(anchor, anchor + hook).replace(event, event + capture)


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--source', required=True, type=Path)
    p.add_argument('--output', required=True, type=Path)
    args = p.parse_args()
    source = args.source.read_text()
    proposed = patched_source(source)
    compile(proposed, 'proposed-rtl-weather.py', 'exec')
    patch = ''.join(difflib.unified_diff(source.splitlines(True), proposed.splitlines(True),
                                        fromfile='rtl_weather.py', tofile='rtl_weather.py', n=0))
    with args.output.open('x') as target:
        target.write(patch)
    print(json.dumps({'applied': False, 'source_sha256': SOURCE_SHA256,
                      'proposed_sha256': hashlib.sha256(proposed.encode()).hexdigest(),
                      'patch_sha256': hashlib.sha256(patch.encode()).hexdigest()}))
