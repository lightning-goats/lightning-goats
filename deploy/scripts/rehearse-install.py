#!/usr/bin/env python3
"""Root-owned package rehearsal in a NEW loopback-only network namespace.

Run only against an explicitly trusted archive. Never installs host services,
creates users, reads production credentials or modifies host networking.
"""
import sys
sys.dont_write_bytecode = True

import argparse
import base64
from contextlib import ExitStack
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import json
import os
from pathlib import Path
import pwd
import shutil
import socket
import subprocess
import tempfile
import threading
import time
import urllib.request
import uuid

SPEC = importlib.util.spec_from_file_location("release", Path(__file__).with_name("smoke-release.py"))
RELEASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RELEASE)


def require_isolation():
    if os.geteuid() != 0:
        raise ValueError("run through sudo unshare --net; root is needed only for isolated ownership setup")
    if os.readlink("/proc/self/ns/net") == os.readlink("/proc/1/ns/net"):
        raise ValueError("refusing the host network namespace")
    interfaces = json.loads(subprocess.check_output(["ip", "-j", "link", "show"]))
    if [link["ifname"] for link in interfaces] != ["lo"]:
        raise ValueError("namespace must contain only loopback")
    subprocess.run(["ip", "link", "set", "lo", "up"], check=True)


def identity(name):
    user = pwd.getpwnam(name)
    if user.pw_uid == 0 or user.pw_gid == 0:
        raise ValueError("runtime identity must be non-root")
    return user


def command_as(user, command):
    return ["setpriv", f"--reuid={user.pw_uid}", f"--regid={user.pw_gid}",
            "--clear-groups", "--no-new-privs", "--", *map(str, command)]


def private_directory(path, user):
    path.mkdir(mode=0o700)
    os.chown(path, user.pw_uid, user.pw_gid)


def root_file(path, data, group=0, mode=0o644):
    path.write_text(data)
    os.chown(path, 0, group)
    path.chmod(mode)


def replace_once(text, old, new):
    if text.count(old) != 1:
        raise ValueError(f"shipped fixture changed: {old}")
    return text.replace(old, new)


def free_port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def request(url, post=False):
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    req = urllib.request.Request(url, data=b"" if post else None)
    with opener.open(req, timeout=10) as response:
        body = response.read(64 * 1024)
        return json.loads(body) if body else None


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def await_health(process, url):
    for _ in range(100):
        if process.poll() is not None:
            raise ValueError("installed process exited during startup")
        try:
            request(url + "/healthz")
            return
        except (OSError, ValueError):
            time.sleep(0.1)
    raise ValueError("installed process did not become healthy")


def process_evidence(process, user):
    fields = dict(line.split(":", 1) for line in Path(f"/proc/{process.pid}/status").read_text().splitlines() if ":" in line)
    assert set(map(int, fields["Uid"].split())) == {user.pw_uid}
    assert set(map(int, fields["Gid"].split())) == {user.pw_gid}
    assert int(fields["CapEff"].strip(), 16) == 0
    assert fields["NoNewPrivs"].strip() == "1"
    return {"uid": user.pw_uid, "gid": user.pw_gid, "effective_capabilities": 0, "no_new_privileges": True}


def rehearse(archive, source, daemon_user, gateway_user):
    require_isolation()
    if daemon_user.pw_uid == gateway_user.pw_uid or daemon_user.pw_gid == gateway_user.pw_gid:
        raise ValueError("daemon and gateway must use distinct users and groups")
    os.umask(0o077)
    with tempfile.TemporaryDirectory(prefix="lg-install-") as directory, ExitStack() as cleanup:
        root = Path(directory)
        root.chmod(0o755)
        extracted = root / "package"
        extracted.mkdir()
        RELEASE.verify_archive(archive, source, extracted) # execute no archive payload as root
        installed = root / "bin"
        installed.mkdir(mode=0o755)
        installed.chmod(0o755)
        binary_hashes = {}
        for name in sorted(RELEASE.BINARIES):
            target = installed / name
            shutil.copyfile(extracted / name, target)
            target.chmod(0o755)
            assert target.stat().st_uid == 0
            binary_hashes[name] = hashlib.sha256(target.read_bytes()).hexdigest()
        daemon_state, gateway_state = root / "daemon-state", root / "gateway-state"
        private_directory(daemon_state, daemon_user)
        private_directory(gateway_state, gateway_user)
        credentials = {}
        for name, user, secrets in [
            ("daemon", daemon_user, ["strike-api-key", "strike-webhook-secret"]),
            ("gateway", gateway_user, ["openhab-token"]),
        ]:
            path = root / f"{name}-credentials"
            path.mkdir(mode=0o750)
            path.chmod(0o750)
            os.chown(path, 0, user.pw_gid)
            for secret in secrets:
                root_file(path / secret, "synthetic-install-only", user.pw_gid, 0o440)
            credentials[name] = path

        commands = []
        class Owner(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass
            def do_GET(self):
                item = self.path.split("/")[-2]
                values = {"FeederOverride":"OFF", "LightningGoatsCanaryRemoteEnabled":"ON",
                          "LightningGoatsCanaryAck": commands[-1] if commands else "UNDEF",
                          "AmbientWeatherWS2902A_WeatherDataWs2902a_Temperature":"20 °C"}
                self.send_response(200)
                self.end_headers()
                self.wfile.write(values.get(item,"UNDEF").encode())
            def do_POST(self):
                if self.path != "/rest/items/LightningGoatsCanaryRequest":
                    self.send_error(404)
                    return
                if self.headers.get("Authorization") != "Basic " + base64.b64encode(b"synthetic-install-only:").decode():
                    self.send_error(401)
                    return
                length = int(self.headers.get("Content-Length", "0"))
                if not 1 <= length <= 128:
                    self.send_error(413)
                    return
                value = str(uuid.UUID(self.rfile.read(length).decode()))
                commands.append(value)
                self.send_response(200)
                self.end_headers()
        owner = ThreadingHTTPServer(("127.0.0.1",0), Owner)
        thread = threading.Thread(target=owner.serve_forever, daemon=True)
        thread.start()
        cleanup.callback(owner.server_close)
        cleanup.callback(owner.shutdown)
        daemon_port, gateway_port = free_port(), free_port()
        while gateway_port == daemon_port:
            gateway_port = free_port()
        daemon_url = f"http://127.0.0.1:{daemon_port}"
        gateway_url = f"http://127.0.0.1:{gateway_port}"
        daemon_config = (extracted / "deploy/config.canary.toml.example").read_text()
        for old, new in [
            ('127.0.0.1:8788', f'127.0.0.1:{daemon_port}'),
            ('sqlite:///var/lib/lightning-goats/lightning-goats-canary.db',f'sqlite://{daemon_state}/daemon.db'),
            ('https://api.dev.strike.me/', 'https://127.0.0.1:9/'),
            ('http://10.8.0.6:8790/', gateway_url + '/'),
            ('interface_info_enabled = true', 'interface_info_enabled = false'),
            ('weather_enabled = true', 'weather_enabled = false'),
        ]:
            daemon_config = replace_once(daemon_config, old, new)
        gateway_config = (extracted / "deploy/gateway/config.canary.toml.example").read_text()
        for old, new in [
            ('10.8.0.6:8790', f'127.0.0.1:{gateway_port}'),
            ('sqlite:///var/lib/lightning-goats-gateway-canary/gateway.db', f'sqlite://{gateway_state}/gateway.db'),
            ('http://127.0.0.1:8080/', f'http://127.0.0.1:{owner.server_port}/'),
        ]:
            gateway_config = replace_once(gateway_config,old,new)
        configs = {"daemon":root / "daemon.toml", "gateway":root / "gateway.toml"}
        root_file(configs["daemon"], daemon_config, daemon_user.pw_gid, 0o640)
        root_file(configs["gateway"], gateway_config, gateway_user.pw_gid, 0o640)
        for name,user,other in [("daemon",daemon_user,"gateway"),("gateway",gateway_user,"daemon")]:
            # Each runtime can read only its own credentials and cannot alter code/config.
            program = "import os,sys; assert all(not os.access(p,os.W_OK) for p in sys.argv[1:5]); assert os.access(sys.argv[5],os.R_OK); assert not os.access(sys.argv[6],os.R_OK)"
            own_secret = credentials[name] / ("strike-api-key" if name == "daemon" else "openhab-token")
            other_secret = credentials[other] / ("openhab-token" if name == "daemon" else "strike-api-key")
            subprocess.run(command_as(user,[sys.executable,"-c",program,*[installed/n for n in sorted(RELEASE.BINARIES)],configs[name],own_secret,other_secret]),check=True,cwd="/")
        def start(name,user,binary,url):
            log = cleanup.enter_context((root / f"{name}.log").open("ab"))
            env = {"PATH":os.defpath, "CREDENTIALS_DIRECTORY":str(credentials[name]), "TOKIO_WORKER_THREADS":"2", "RUST_LOG":"warn"}
            process = subprocess.Popen(command_as(user,[installed / binary,"--config",configs[name]]),cwd="/",env=env,stdout=log,stderr=log)
            cleanup.callback(stop,process)
            try:
                await_health(process,url)
            except Exception:
                log.flush()
                # Synthetic-only bounded diagnostics; no production config/credentials involved.
                print((root / f"{name}.log").read_text()[-4096:],file=sys.stderr)
                raise
            return process
        gateway = start("gateway",gateway_user,"lightning-goats-gateway",gateway_url)
        daemon = start("daemon",daemon_user,"lightning-goatsd",daemon_url)
        evidence = {"source_commit":source,"binary_sha256":binary_hashes,"scope":"isolated installed release with synthetic credentials; no systemd/TLS/live acceptance"}
        evidence["daemon"] = process_evidence(daemon,daemon_user)
        evidence["gateway"] = process_evidence(gateway,gateway_user)
        status = request(daemon_url + "/api/v1/status")
        assert status["feed_credit_sats"] == 0 and status["gateway_reachable"]
        assert status["temperature_f"] == 68.0
        for user in ["herd","dexter","rowan","cosmo","newton","nova"]:
            assert request(daemon_url + "/.well-known/lnurlp/" + user)["tag"] == "payRequest"
        request_id = str(uuid.uuid4())
        for _ in range(2):
            result = request(gateway_url + "/v1/feeder/request/" + request_id,post=True)
            assert result["request_id"] == request_id and result["status"] == "confirmed"
        assert commands == [request_id]
        stop(daemon)
        stop(gateway)
        gateway = start("gateway",gateway_user,"lightning-goats-gateway",gateway_url)
        daemon = start("daemon",daemon_user,"lightning-goatsd",daemon_url)
        assert request(gateway_url + "/v1/feeder/request/" + request_id,post=True)["status"] == "confirmed"
        assert commands == [request_id]
        assert request(daemon_url + "/api/v1/status")["feed_credit_sats"] == 0
        for path,user in [(daemon_state/"daemon.db",daemon_user),(gateway_state/"gateway.db",gateway_user)]:
            assert path.stat().st_uid == user.pw_uid
            assert path.stat().st_mode & 0o777 == 0o600
        evidence.update(mock_owner_commands=1, restart_idempotency=True, root_owned_immutable_code_config=True, separated_credentials=True, all_six_discovery_routes=True, namespace_interfaces=["lo"])
        return evidence


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive",type=Path)
    parser.add_argument("source_commit")
    parser.add_argument("--daemon-user",default="daemon")
    parser.add_argument("--gateway-user",default="bin")
    args = parser.parse_args()
    print(json.dumps(rehearse(args.archive.resolve(strict=True),args.source_commit,identity(args.daemon_user),identity(args.gateway_user)),sort_keys=True,indent=2))


if __name__ == "__main__":
    main()
