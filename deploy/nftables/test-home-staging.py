#!/usr/bin/env python3
"""Isolated staging-filter test derived from PR57 packet fixtures. Run only through sudo unshare --net -- python3.
Creates virtual links in its disposable namespace; never connects to WireGuard.
"""
import argparse
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import time


def run(*args, **kwargs):
    return subprocess.run(args, check=True, text=True, capture_output=True, **kwargs).stdout


parser = argparse.ArgumentParser()
parser.add_argument('--held', action='store_true')
args = parser.parse_args()
gateway_port = 8791 if args.held else 8790
policy = 'home-held-staging-peer.nft.example' if args.held else 'home-staging-peer.nft.example'

if os.readlink('/proc/self/ns/net') == os.readlink('/proc/1/ns/net'):
    raise SystemExit('Refusing the host network namespace')
run('ip', 'link', 'add', 'wg0', 'type', 'veth', 'peer', 'name', 'inject')
for link in ['wg0', 'inject']:
    run('ip', 'link', 'set', link, 'up')
run('ip', 'addr', 'add', '10.8.0.6/32', 'dev', 'wg0')
run('ip', '-6', 'addr', 'add', 'fd5e:6df:9c82::6/128', 'dev', 'wg0', 'nodad')
run('nft', '-f', '-', input='''table inet broad_existing {
 chain input { type filter hook input priority 0; policy accept; }
 chain forward { type filter hook forward priority 0; policy accept; }
}
table ip docker_like {
 chain prerouting { type nat hook prerouting priority -100; policy accept;
 tcp dport 80 dnat to 172.30.0.2:80
 }
}
''')
run('nft', '-f', str(Path(__file__).with_name(policy)))
run('ip', 'route', 'add', '10.8.0.0/24', 'dev', 'wg0')
run('ip', '-6', 'route', 'add', 'fd5e:6df:9c82::/64', 'dev', 'wg0')
# Add no permissions; count packets that survive the guard into input.
run('nft', 'add', 'rule', 'inet', 'broad_existing', 'input', 'iifname', 'wg0', 'counter')
dstmac = bytes.fromhex(json.loads(run('ip', '-j', 'link', 'show', 'wg0'))[0]['address'].replace(':', ''))
srcmac = bytes.fromhex(json.loads(run('ip', '-j', 'link', 'show', 'inject'))[0]['address'].replace(':', ''))
sock = socket.socket(socket.AF_PACKET, socket.SOCK_RAW)
sock.bind(('inject', 0))


def checksum(data):
    words = struct.unpack('!%dH' % (len(data)//2), data)
    s = sum(words)
    while s >> 16:
        s = (s & 65535) + (s >> 16)
    return (~s) & 65535


def packet(source, port, ipv6=False, ack=False, source_port=45000, destination=None):
    destination = destination or ('fd5e:6df:9c82::6' if ipv6 else '10.8.0.6')
    family = socket.AF_INET6 if ipv6 else socket.AF_INET
    src, dst = (socket.inet_pton(family, a) for a in (source, destination))
    tcp = struct.pack('!HHIIBBHHH', source_port, port, 1, 1 if ack else 0, 80, 16 if ack else 2, 1024, 0, 0)
    pseudo = src + dst + (struct.pack('!I3xB', len(tcp), 6) if ipv6 else struct.pack('!BBH', 0, 6, len(tcp)))
    tcp = tcp[:16] + struct.pack('!H', checksum(pseudo + tcp)) + tcp[18:]
    if ipv6:
        header = struct.pack('!IHBB', 6 << 28, len(tcp), 6, 64) + src + dst
    else:
        header = struct.pack('!BBHHHBBH', 69, 0, 40, 1, 0, 64, 6, 0) + src + dst
        header = header[:10] + struct.pack('!H', checksum(header)) + header[12:]
    return dstmac + srcmac + struct.pack('!H', 0x86dd if ipv6 else 0x800) + header + tcp


def drops():
    data = json.loads(run('nft', '-j', 'list', 'table', 'inet', 'lg_home_staging_peer'))
    return sum(expr['counter']['packets'] for entry in data['nftables'] for expr in entry.get('rule', {}).get('expr', []) if 'counter' in expr)


def input_count():
    data = json.loads(run('nft', '-j', 'list', 'table', 'inet', 'broad_existing'))
    return sum(expr['counter']['packets'] for entry in data['nftables'] for expr in entry.get('rule', {}).get('expr', []) if 'counter' in expr)


cases = [('gateway', '10.8.0.12', gateway_port, False, False, False),
         ('other canary denied', '10.8.0.12', 8790 if args.held else 8791, False, False, True),
         ('production denied', '10.8.0.12', 8789, False, False, True),
         ('SSH', '10.8.0.12', 22, False, False, True),
         ('OpenHAB', '10.8.0.12', 8080, False, False, True),
         ('Docker DNAT', '10.8.0.12', 80, False, False, True),
         ('legacy administrator preserved', '10.8.0.10', 22, False, False, False),
         ('legacy hub preserved', '10.8.0.1', 8080, False, False, False),
         ('ACK bypass', '10.8.0.12', 8080, False, True, True),
         ('legacy IPv6 preserved (staging key has no IPv6)', 'fd5e:6df:9c82::1', 8080, True, False, False)]
for name, source, port, ipv6, ack, denied in cases:
    before = drops()
    input_before = input_count()
    sock.send(packet(source, port, ipv6, ack))
    time.sleep(.05)
    observed = drops() - before
    assert observed == int(denied), (name, observed, denied)
    assert input_count() - input_before == int(not denied), (name, 'input counter mismatch')
    print('PASS', name, 'drop' if denied else 'passes early guard')
# A future Docker mapping of the admitted port must still hit the forward drop.
run('ip', 'link', 'add', 'containerout', 'type', 'dummy')
run('ip', 'link', 'set', 'containerout', 'up')
run('ip', 'addr', 'add', '172.30.0.1/24', 'dev', 'containerout')
Path('/proc/sys/net/ipv4/ip_forward').write_text('1')
run('nft', 'add', 'rule', 'ip', 'docker_like', 'prerouting', 'tcp', 'dport', str(gateway_port), 'dnat', 'to', f'172.30.0.2:{gateway_port}')
before = drops()
sock.send(packet('10.8.0.12', gateway_port, source_port=45001))
time.sleep(.05)
assert drops() - before == 1, 'admitted-port forwarding bypass'
data = json.loads(run('nft', '-j', 'list', 'table', 'inet', 'lg_home_staging_peer'))
forwarded = [e['rule'] for e in data['nftables'] if e.get('rule', {}).get('chain') == 'forwarded']
assert sum(x['counter']['packets'] for r in forwarded for x in r['expr'] if 'counter' in x) == 1
print('PASS admitted port DNAT is blocked in forward chain')
print('PASS initial packet cases; peer authentication is separately tested in PR62, no home activation')

# Route/LAN alternative drops before routing and DNAT.
before = drops()
sock.send(packet('10.8.0.12', gateway_port, source_port=45002, destination='172.30.0.2'))
time.sleep(.05)
assert drops() - before == 1, 'alternate destination bypass'
print('PASS alternate destination blocked before routing')
# A future redirect of the admitted port to a different local service is denied.
run('nft', 'insert', 'rule', 'ip', 'docker_like', 'prerouting', 'tcp', 'dport', str(gateway_port), 'dnat', 'to', '10.8.0.6:8080')
before, input_before = drops(), input_count()
sock.send(packet('10.8.0.12', gateway_port, source_port=45003))
time.sleep(.05)
assert drops() - before == 1 and input_count() == input_before, 'local DNAT bypass'
print('PASS admitted port DNAT to other local service blocked')
# Delete only the staging table; independently existing tables remain intact.
run('nft', 'delete', 'table', 'inet', 'lg_home_staging_peer')
for family, table in [('inet', 'broad_existing'), ('ip', 'docker_like')]:
    run('nft', 'list', 'table', family, table)
print('PASS table-only rollback preserves other managers')
