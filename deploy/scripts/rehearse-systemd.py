#!/usr/bin/env python3
"""Rehearse shipped canary sandboxes and encrypted synthetic credentials.

Requires a trusted archive, an initialized systemd host credential key and a NEW
loopback-only network namespace. Creates temporary uniquely named system units;
never enables them, reads production credentials or changes host networking.
"""
import sys
sys.dont_write_bytecode = True

import argparse
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import os
import platform
from pathlib import Path
import shutil
import subprocess
import time
import uuid

SPEC = importlib.util.spec_from_file_location("install", Path(__file__).with_name("rehearse-install.py"))
INSTALL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(INSTALL)
TEMPLATES = Path(__file__).resolve().parents[1] / "systemd"


def service_properties(path):
    """Retain repeated properties and reject syntax this harness cannot preserve."""
    section = None
    properties = []
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith(("#", ";")):
            continue
        if line.startswith("[") and line.endswith("]"):
            section = line[1:-1]
        elif section == "Service":
            if "=" not in line or line.endswith("\\"):
                raise ValueError("unsupported service template syntax")
            properties.append(tuple(line.split("=", 1)))
    if not properties:
        raise ValueError("missing service properties")
    return properties


def remove_directory(path):
    if path.exists():
        shutil.rmtree(path)


def command(args, check=True):
    return subprocess.run(args, check=check, text=True, capture_output=True,
                          env={"PATH":os.defpath}, timeout=30)


class UnitProcess:
    def __init__(self, unit):
        self.unit = unit

    def properties(self):
        result = command(["systemctl", "show", self.unit, "--property=MainPID,ActiveState,ExecMainStatus,NRestarts"], check=False)
        return dict(line.split("=",1) for line in result.stdout.splitlines() if "=" in line)

    @property
    def pid(self):
        return int(self.properties().get("MainPID", "0"))

    def poll(self):
        properties = self.properties()
        if properties.get("ActiveState") in ["active", "activating", "deactivating"]:
            return None
        return int(properties.get("ExecMainStatus", "0"))

    def terminate(self):
        command(["systemctl", "stop", self.unit], check=False)

    def kill(self):
        command(["systemctl", "kill", "--signal=KILL", self.unit], check=False)

    def wait(self, timeout):
        deadline = time.monotonic() + timeout
        while self.poll() is None:
            if time.monotonic() >= deadline:
                raise subprocess.TimeoutExpired(self.unit, timeout)
            time.sleep(0.1)
        return self.poll()


# Executed by ExecStartPre under the same sandbox, user and credential mounts.
PROBE = r'''
import json, os
from pathlib import Path
import sys
import re
import urllib.request
state, config, installed, expected, namespace, tmp_marker, home_marker, other_unit = sys.argv[1:]
credentials = Path(os.environ["CREDENTIALS_DIRECTORY"])
names = expected.split(",")
assert sorted(p.name for p in credentials.iterdir()) == sorted(names)
assert all((credentials / name).read_text() == "synthetic-install-only" for name in names)
assert not os.access(Path("/run/credentials") / other_unit, os.R_OK)
assert os.geteuid() != 0 and os.getegid() != 0
assert not os.access(config, os.W_OK)
assert all(not os.access(p, os.W_OK) for p in Path(installed).iterdir())
assert os.access(state, os.W_OK)
assert os.readlink("/proc/self/ns/net") == namespace
match = re.search(r'\[gateway\]\s+url = "([^"]+)"', Path(config).read_text())
if match:
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with opener.open(match[1] + "v1/feeder/override", timeout=5) as response:
        assert response.status == 200
assert not Path(tmp_marker).exists()
assert not os.access(home_marker, os.R_OK)
status = dict(line.split(":",1) for line in Path("/proc/self/status").read_text().splitlines() if ":" in line)
assert status["NoNewPrivs"].strip() == "1"
assert int(status["CapEff"].strip(),16) == 0
assert not status["Groups"].split() or set(map(int,status["Groups"].split())) == {os.getegid()}
result = {"credential_names": sorted(names), "credential_values_verified": True,
          "other_unit_credentials_inaccessible": True, "network_namespace_verified": True,
          "private_tmp_verified": True, "protect_home_verified": True,
          "root_owned_code_config_nonwritable": True, "state_writable": True}
Path(state, "sandbox-probe.json").write_text(json.dumps(result,sort_keys=True))
'''


class SystemdLauncher:
    def __init__(self, root, cleanup):
        self.root, self.cleanup = root, cleanup
        self.prefix = "lg-rehearsal-" + uuid.uuid4().hex
        self.namespace = os.readlink("/proc/self/ns/net")
        self.units = {}
        self.all_units = []
        cleanup.callback(self.verify_units_removed)
        self.starts = []
        self.other_active_at_start = {}
        self.encrypted = {}
        self.template_hashes = {}
        self.sandbox_properties = {}
        # Existing staging key is required; this harness never creates/replaces it.
        key = Path("/var/lib/systemd/credential.secret")
        if not key.is_file() or key.is_symlink() or key.stat().st_uid != 0 or key.stat().st_mode & 0o077:
            raise ValueError("initialize/recover the protected staging credential store first")
        self.tmp_marker = Path("/tmp") / self.prefix
        self.tmp_marker.touch(mode=0o644, exist_ok=False)
        cleanup.callback(self.tmp_marker.unlink)
        for role in ["daemon", "gateway"]:
            # Reserve only unique paths; never adopt an existing directory.
            for base in ["/etc", "/run"]:
                path = Path(base) / f"{self.prefix}-{role}"
                path.mkdir(mode=0o755)
                path.chmod(0o755)
                cleanup.callback(remove_directory, path)
            if self.state_path(role).exists():
                raise ValueError("unexpected preexisting rehearsal state directory")
            cleanup.callback(remove_directory, self.state_path(role))
        self.probe = root / "sandbox-probe.py"
        INSTALL.root_file(self.probe, PROBE)

    def verify_units_removed(self):
        for unit in self.all_units:
            for _ in range(50):
                if command(["systemctl","show",unit,"--property=LoadState","--value"],check=False).stdout.strip() == "not-found":
                    break
                time.sleep(0.1)
            else:
                raise ValueError(f"temporary unit was not collected: {unit}")

    def state_path(self, role):
        return Path("/var/lib") / f"{self.prefix}-{role}"

    def config_path(self, role):
        return Path("/etc") / f"{self.prefix}-{role}" / "config.toml"

    def prepare(self, credentials, installed, configs):
        self.installed = installed
        self.selinux_labels = {}
        if Path("/sys/fs/selinux/enforce").exists():
            # Match the default labels of the shipped /usr/local/bin destination
            # on only these disposable copies. Never alter system policy.
            for binary in sorted(installed.iterdir()):
                label = command(["matchpathcon", "-n", f"/usr/local/bin/{binary.name}"]).stdout.strip()
                command(["chcon", label, str(binary)])
                self.selinux_labels[binary.name] = label
        encrypted = self.root / "encrypted"
        encrypted.mkdir(mode=0o700)
        for role, directory in credentials.items():
            self.encrypted[role] = {}
            for plain in sorted(directory.iterdir()):
                cipher = encrypted / f"{role}-{plain.name}"
                command(["systemd-creds", "encrypt", "--with-key=host", f"--name={plain.name}", str(plain), str(cipher)])
                cipher.chmod(0o600)
                self.encrypted[role][plain.name] = cipher
            # All values are synthetic; prove subsequent startup uses decryption.
            shutil.rmtree(directory)
        self.negative_credential_checks()

    def launch_unit(self, unit, properties, executable, expected_failure=False):
        process = UnitProcess(unit)
        if unit not in self.all_units:
            self.all_units.append(unit)
        self.cleanup.callback(INSTALL.stop, process)
        arguments = ["systemd-run", "--quiet", "--no-ask-password", f"--unit={unit}"]
        if not any(key == "CollectMode" for key,value in properties):
            properties = [*properties, ("CollectMode", "inactive-or-failed")]
        for key,value in properties:
            arguments.append(f"--property={key}={value}")
        result = command([*arguments, "--", *map(str,executable)], check=False)
        if result.returncode and not expected_failure:
            raise ValueError(f"systemd unit launch failed: {(result.stdout + result.stderr)[-2048:]}")
        return process

    def negative_credential_checks(self):
        original = self.encrypted["gateway"]["openhab-token"]
        # Corruption is guaranteed invalid ciphertext, independent of base64 framing.
        damaged = original.with_name("damaged")
        damaged.write_bytes(b"invalid-encrypted-credential")
        self.negative_results = {}
        for label,name,path in [("wrong_name", "wrong-name", original), ("tampered", "openhab-token", damaged)]:
            unit = f"{self.prefix}-{label}.service"
            process = self.launch_unit(unit, [
                ("Type","oneshot"), ("RemainAfterExit","yes"), ("User","daemon"),
                ("NetworkNamespacePath",f"/proc/{os.getpid()}/ns/net"),
                ("LoadCredentialEncrypted",f"{name}:{path}"),
                # Keep failed unit until inspected; reset explicitly afterwards.
                ("CollectMode","inactive"),
            ], ["/usr/bin/true"], expected_failure=True)
            try:
                process.wait(timeout=10)
                properties = process.properties()
                if properties.get("ExecMainStatus") != "243":
                    raise ValueError(f"{label} did not fail at CREDENTIALS: {properties}")
                self.negative_results[label] = "rejected_before_exec_status_243"
            finally:
                process.terminate()
                command(["systemctl", "reset-failed", unit],check=False)

    def start(self, role, user, binary, config):
        unit = f"{self.prefix}-{role}.service"
        if role in self.units:
            # Wait for --collect removal before reusing the same unit after stop.
            for _ in range(50):
                if command(["systemctl","show",unit,"--property=LoadState","--value"],check=False).stdout.strip() == "not-found":
                    break
                time.sleep(0.1)
        filename = "lightning-goats-canary.service" if role == "daemon" else "lightning-goats-gateway-canary.service"
        template = TEMPLATES / filename
        self.template_hashes[filename] = hashlib.sha256(template.read_bytes()).hexdigest()
        changes = {"User":user.pw_name, "Group":str(user.pw_gid),
                   "StateDirectory":f"{self.prefix}-{role}",
                   "RuntimeDirectory":f"{self.prefix}-{role}",
                   "ConfigurationDirectory":f"{self.prefix}-{role}",
                   "Environment":"RUST_LOG=warn TOKIO_WORKER_THREADS=2"}
        props = []
        for key,value in service_properties(template):
            if key == "ExecStart":
                continue
            if key == "LoadCredentialEncrypted":
                name = value.split(":",1)[0]
                value = f"{name}:{self.encrypted[role][name]}"
            props.append((key,changes.get(key,value)))
        self.sandbox_properties[role] = [f"{key}={value}" for key,value in props if key not in changes and key != "LoadCredentialEncrypted"]
        other = "daemon" if role == "gateway" else "gateway"
        probe_args = ["/usr/bin/python3", str(self.probe), str(self.state_path(role)), str(config), str(self.installed),
                      ",".join(sorted(self.encrypted[role])), self.namespace, str(self.tmp_marker),
                      str(Path(__file__).resolve()), f"{self.prefix}-{other}.service"]
        # All paths are controlled without whitespace; namespace contains brackets.
        props.extend([
            ("NetworkNamespacePath",f"/proc/{os.getpid()}/ns/net"),
            ("ExecStartPre"," ".join(probe_args)),
            ("StandardOutput",f"append:{self.root}/{role}.log"),
            ("StandardError",f"append:{self.root}/{role}.log"),
        ])
        self.other_active_at_start[role] = other in self.units and self.units[other].properties().get("ActiveState") == "active"
        process = self.launch_unit(unit, props, [binary,"--config",config])
        self.units[role] = process
        return process

    def started(self, role, process):
        if process.properties().get("NRestarts") != "0":
            raise ValueError("automatic service restart masked a startup failure")
        state = self.state_path(role)
        runtime = Path("/run") / f"{self.prefix}-{role}"
        assert state.stat().st_mode & 0o777 == 0o700
        assert runtime.stat().st_mode & 0o777 == 0o700
        assert self.config_path(role).stat().st_uid == 0
        record = {"role":role, "automatic_restarts":0, "state_runtime_mode":"0700",
                  "other_service_active_during_credential_probe":self.other_active_at_start[role]}
        context = Path(f"/proc/{process.pid}/attr/current")
        if self.selinux_labels:
            record["selinux_process_context"] = context.read_text().strip().strip("\0")
        self.starts.append(record)

    def evidence(self):
        for role in ["daemon", "gateway"]:
            if not any(record["role"] == role and record["other_service_active_during_credential_probe"] for record in self.starts):
                raise ValueError("credential isolation must be checked in both directions with the other service running")
        probes = {}
        for role, process in self.units.items():
            if process.properties().get("NRestarts") != "0":
                raise ValueError("automatic service restart masked a failure")
            probes[role] = json.loads((self.state_path(role) / "sandbox-probe.json").read_text())
        return {"scope":"actual transient systemd canary sandboxes and encrypted synthetic credentials; no live acceptance",
                "systemd_version":command(["systemctl","--version"]).stdout.splitlines()[0],
                "template_sha256":self.template_hashes, "temporary_binary_selinux_labels":self.selinux_labels, "preserved_service_properties":self.sandbox_properties,
                "systemd_sandbox_probes":probes,"negative_encrypted_credentials":self.negative_results,
                "credential_separation_checked_with_other_service_active":True,"service_starts":self.starts,"plaintext_source_credentials_removed":True,"automatic_service_restarts":0}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive",type=Path)
    parser.add_argument("source_commit")
    parser.add_argument("--daemon-user",default="daemon")
    parser.add_argument("--gateway-user",default="bin")
    args = parser.parse_args()
    evidence = INSTALL.rehearse(args.archive.resolve(strict=True),args.source_commit,
                               INSTALL.identity(args.daemon_user),INSTALL.identity(args.gateway_user),SystemdLauncher)
    # Success is emitted only after the context has stopped units and removed files.
    evidence["temporary_installation_removed"] = True
    evidence["observed_at_utc"] = datetime.now(timezone.utc).isoformat()
    evidence["environment"] = platform.platform()
    evidence["archive_sha256"] = hashlib.sha256(args.archive.read_bytes()).hexdigest()
    evidence["harness_sha256"] = {name:hashlib.sha256(Path(__file__).with_name(name).read_bytes()).hexdigest()
                                  for name in ["rehearse-systemd.py","rehearse-install.py","smoke-release.py"]}
    if Path("/sys/fs/selinux/enforce").exists():
        evidence["selinux_enforcing"] = Path("/sys/fs/selinux/enforce").read_text().strip() == "1"
    print(json.dumps(evidence,sort_keys=True,indent=2))


if __name__ == "__main__":
    main()
