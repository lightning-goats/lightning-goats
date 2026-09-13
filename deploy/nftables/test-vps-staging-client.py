#!/usr/bin/env python3
"""Isolated real TCP transition test; run through sudo unshare --net."""
import json,os,pathlib,socket,subprocess,time

def run(*args,**kw):
 return subprocess.run(args,check=True,text=True,capture_output=True,timeout=10,**kw).stdout
assert json.loads(run('ip','-j','link'))[0]['ifname']=='lo'
assert len(json.loads(run('ip','-j','link')))==1
peer=subprocess.Popen(['unshare','--net','--','sleep','120'])
try:
 for _ in range(50):
  if os.readlink('/proc/%s/ns/net'%peer.pid)!=os.readlink('/proc/self/ns/net'):break
  time.sleep(.02)
 else:raise RuntimeError('peer namespace missing')
 def remote(*args):return run('nsenter','-t',str(peer.pid),'-n',*args)
 run('ip','link','add','wg-inspect','type','veth','peer','name','testhome')
 run('ip','link','set','testhome','netns',str(peer.pid))
 run('ip','addr','add','10.77.0.12/32','dev','wg-inspect')
 run('ip','link','set','wg-inspect','up');run('ip','link','set','lo','up')
 run('ip','route','add','10.77.0.6/32','dev','wg-inspect')
 remote('ip','addr','add','10.77.0.6/32','dev','testhome')
 remote('ip','link','set','testhome','up');remote('ip','link','set','lo','up')
 remote('ip','route','add','10.77.0.12/32','dev','testhome')
 server_code="import socket,threading,time\n"
 server_code += """def serve(port):
 s=socket.socket();s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1);s.bind(('10.77.0.6',port));s.listen()
 def echo(c):
  with c:
   while True:
    data=c.recv(100)
    if not data:return
    c.sendall(data)
 while True:
  c,a=s.accept();threading.Thread(target=echo,args=(c,),daemon=True).start()
for port in [22,7543,8790]:threading.Thread(target=serve,args=(port,),daemon=True).start()
time.sleep(110)
"""
 server=subprocess.Popen(['nsenter','-t',str(peer.pid),'-n','python3','-c',server_code],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 try:
  old='table inet lg_inspection { chain input { type filter hook input priority 0; policy drop; iifname "lo" accept; ip saddr 10.77.0.6 tcp sport {22,7543} ct state established accept; } chain output { type filter hook output priority 0; policy drop; oifname "lo" accept; ip daddr 10.77.0.6 tcp dport {22,7543} accept; } chain forward { type filter hook forward priority 0; policy drop; } }'
  old = old.replace(' } chain', ' }; chain').replace(' } }', ' }; }')
  run('nft','-f','-',input=old)
  sockets=[]
  for port in [22,7543]:
   for _ in range(40):
    try:
     c=socket.create_connection(('10.77.0.6',port),.2);break
    except OSError:time.sleep(.05)
   else:raise RuntimeError('positive listener missing')
   c.sendall(b'before');assert c.recv(6)==b'before';sockets.append(c)
  def denied(port):
   try:
    with socket.create_connection(('10.77.0.6',port),.25):return False
   except OSError:return True
  assert denied(8790)
  candidate=pathlib.Path(__file__).with_name('vps-staging-client.nft.example').read_text().replace('__TABLE__','lg_inspection').replace('__INTERFACE__','wg-inspect').replace('__GATEWAY_IPV4__','10.77.0.6')
  run('nft','--check','-f','-',input=candidate);run('nft','-f','-',input=candidate)
  for c in sockets:
   c.settimeout(.25)
   try:
    c.sendall(b'after');data=c.recv(5)
    assert not data,'existing administrative session survived'
   except OSError:pass
   c.close()
  assert denied(22) and denied(7543)
  with socket.create_connection(('10.77.0.6',8790),1) as c:
   c.sendall(b'gateway');assert c.recv(7)==b'gateway'
  run('nft','-f','-',input='delete table inet lg_inspection\n'+old)
  for port in [22,7543]:
   with socket.create_connection(('10.77.0.6',port),1) as c:
    c.sendall(b'restored');assert c.recv(8)==b'restored'
  assert denied(8790)
  print(json.dumps({'scope':'isolated veth TCP policy test, no WireGuard or live hosts','before':'SSH and Hexmem echoes pass; gateway denied','transition':'existing SSH and Hexmem flows blocked; new SSH and Hexmem blocked; gateway echo passes','rollback':'SSH and Hexmem echoes restored; gateway denied','policy':'candidate byte-for-byte except synthetic destination substitution'}))
 finally:
  server.terminate();server.wait(timeout=5)
finally:
 peer.terminate();peer.wait(timeout=5)
