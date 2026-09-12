#!/usr/bin/env python3
"""Real TCP handshake/rollback test in disposable namespaces, never on home."""
import ctypes
import json
import os
from pathlib import Path
import signal
import socket
import subprocess


def run(*args, **kwargs):
    return subprocess.run(args, check=True, capture_output=True, text=True, **kwargs).stdout


if os.readlink('/proc/self/ns/net') == os.readlink('/proc/1/ns/net'):
    raise SystemExit('Refusing the host network namespace')
parent, child = socket.socketpair()
parent.settimeout(5)
child.settimeout(5)
pid = os.fork()
if pid == 0:
    parent.close()
    try:
        # Ubuntu 22.04's Python 3.10 lacks os.unshare; invoke the same Linux
        # primitive directly. This runs only in the forked disposable child.
        libc = ctypes.CDLL(None, use_errno=True)
        libc.unshare.argtypes = [ctypes.c_int]
        libc.unshare.restype = ctypes.c_int
        if libc.unshare(0x40000000) != 0:  # CLONE_NEWNET from linux/sched.h
            error = ctypes.get_errno()
            raise OSError(error, os.strerror(error))
        child.send(b'ready')
        assert child.recv(100) == b'configure'
        run('ip', 'link', 'set', 'inject', 'up')
        run('ip', 'addr', 'add', '10.8.0.12/24', 'dev', 'inject')
        child.send(b'configured')
        clients = {}
        while True:
            command = child.recv(100).decode()
            if command == 'quit':
                break
            action, value = command.split()
            port = int(value)
            if action == 'connect':
                clients[port] = socket.create_connection(('10.8.0.6', port), timeout=1)
                child.send(b'connected')
            elif action == 'echo':
                clients[port].sendall(b'probe')
                try:
                    response = clients[port].recv(5)
                    child.send(b'echoed' if response == b'probe' else b'bad')
                except TimeoutError:
                    child.send(b'blocked')
            elif action == 'close':
                clients.pop(port).close()
                child.send(b'closed')
        os._exit(0)
    except BaseException as error:
        child.send(('error:' + str(error)).encode())
        os._exit(1)
child.close()
try:
    assert parent.recv(100) == b'ready'
    run('ip', 'link', 'add', 'wg0', 'type', 'veth', 'peer', 'name', 'inject')
    run('ip', 'link', 'set', 'inject', 'netns', str(pid))
    run('ip', 'link', 'set', 'wg0', 'up')
    run('ip', 'addr', 'add', '10.8.0.6/24', 'dev', 'wg0')
    parent.send(b'configure')
    assert parent.recv(100) == b'configured'
    run('nft', '-f', '-', input='''table inet broad_existing {
 chain input { type filter hook input priority 0; policy accept;
 ct state established,related counter accept
 }
 chain forward { type filter hook forward priority 0; policy accept; }
}
''')
    listeners = {}
    connections = {}
    for port in (8080, 8790):
        listener = socket.socket()
        listener.settimeout(3)
        listener.bind(('10.8.0.6', port))
        listener.listen(1)
        listeners[port] = listener

    def connect(port):
        parent.send(f'connect {port}'.encode())
        connection, _ = listeners[port].accept()
        connection.settimeout(1.5)
        connections[port] = connection
        assert parent.recv(100) == b'connected'

    def echo(port, allowed=True):
        parent.send(f'echo {port}'.encode())
        if allowed:
            assert connections[port].recv(5) == b'probe'
            connections[port].sendall(b'probe')
            assert parent.recv(100) == b'echoed'
        else:
            assert parent.recv(100) == b'blocked'
            try:
                received = connections[port].recv(5)
            except TimeoutError:
                received = None
            assert received is None, received

    connect(8080)
    echo(8080)
    print('PASS forbidden-port TCP connection established before policy')
    run('nft', '-f', str(Path(__file__).with_name('home-wg-canary.nft.example')))
    echo(8080, allowed=False)
    print('PASS policy blocks data despite existing established/related accept')
    connect(8790)
    echo(8790)
    print('PASS allowed gateway TCP handshake and data after policy')
    parent.send(b'close 8080')
    assert parent.recv(100) == b'closed'
    connections[8080].close()
    run('nft', 'delete', 'table', 'inet', 'lg_home_wg_canary')
    connect(8080)
    echo(8080)
    existing = json.loads(run('nft', '-j', 'list', 'table', 'inet', 'broad_existing'))
    assert any(e.get('table', {}).get('name') == 'broad_existing' for e in existing['nftables'])
    print('PASS narrow rollback restores connection and preserves existing table')
    parent.send(b'quit')
    _, status = os.waitpid(pid, 0)
    assert status == 0, status
    pid = None
finally:
    if pid is not None:
        os.kill(pid, signal.SIGKILL)
        os.waitpid(pid, 0)
