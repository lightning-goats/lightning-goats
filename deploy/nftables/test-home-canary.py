#!/usr/bin/env python3
"""Isolated raw-packet smoke test. Run only through sudo unshare --net -- python3.
Creates virtual links in its disposable namespace; never connects to WireGuard.
"""
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import time


def run(*args, **kwargs):
    return subprocess.run(args, check=True, text=True, capture_output=True, **kwargs).stdout


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
run('nft', '-f', str(Path(__file__).with_name('home-wg-canary.nft.example')))
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


def packet(source, port, ipv6=False, ack=False, source_port=45000):
    destination = 'fd5e:6df:9c82::6' if ipv6 else '10.8.0.6'
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
    data = json.loads(run('nft', '-j', 'list', 'table', 'inet', 'lg_home_wg_canary'))
    return sum(expr['counter']['packets'] for entry in data['nftables'] for expr in entry.get('rule', {}).get('expr', []) if 'counter' in expr)


cases = [('gateway', '10.8.0.12', 8790, False, False, False),
         ('SSH', '10.8.0.12', 22, False, False, True),
         ('OpenHAB', '10.8.0.12', 8080, False, False, True),
         ('Docker DNAT', '10.8.0.12', 80, False, False, True),
         ('spoofed administrator', '10.8.0.10', 22, False, False, True),
         ('other peer gateway', '10.8.0.1', 8790, False, False, True),
         ('ACK bypass', '10.8.0.12', 8080, False, True, True),
         ('IPv6 gateway', 'fd5e:6df:9c82::12', 8790, True, False, True)]
for name, source, port, ipv6, ack, denied in cases:
    before = drops()
    sock.send(packet(source, port, ipv6, ack))
    time.sleep(.05)
    observed = drops() - before
    assert observed == int(denied), (name, observed, denied)
    print('PASS', name, 'drop' if denied else 'passes early guard')
# A future Docker mapping of the admitted port must still hit the forward drop.
run('ip', 'link', 'add', 'containerout', 'type', 'dummy')
run('ip', 'link', 'set', 'containerout', 'up')
run('ip', 'addr', 'add', '172.30.0.1/24', 'dev', 'containerout')
Path('/proc/sys/net/ipv4/ip_forward').write_text('1')
run('ip', 'route', 'add', '10.8.0.0/24', 'dev', 'wg0')
run('nft', 'add', 'rule', 'ip', 'docker_like', 'prerouting', 'tcp', 'dport', '8790', 'dnat', 'to', '172.30.0.2:8790')
before = drops()
sock.send(packet('10.8.0.12', 8790, source_port=45001))
time.sleep(.05)
assert drops() - before == 1, 'admitted-port forwarding bypass'
data = json.loads(run('nft', '-j', 'list', 'table', 'inet', 'lg_home_wg_canary'))
forwarded = [e['rule'] for e in data['nftables'] if e.get('rule', {}).get('chain') == 'forwarded']
assert sum(x['counter']['packets'] for r in forwarded for x in r['expr'] if 'counter' in x) == 1
print('PASS admitted port DNAT is blocked in forward chain')
print('PASS 9 packet cases; no real WireGuard or home activation tested')
