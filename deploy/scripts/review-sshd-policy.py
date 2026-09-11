#!/usr/bin/env python3
"""Review the observed Fedora SSH include layout without applying any host change.

Copies configuration into private temporary storage, substitutes the candidate
early snippet, and runs sshd syntax/effective-policy checks only. No key copying,
server startup, reload, account/key modification or network change. Unknown
include/service layouts and existing Match blocks require a separate review.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import tempfile

MAIN = Path("/etc/ssh/sshd_config")
SNIPPETS = Path("/etc/ssh/sshd_config.d")
CRYPTO = Path("/etc/crypto-policies/back-ends/opensshserver.config")
OPTIONS = Path("/etc/sysconfig/sshd")
TARGET = "00-local-hardening.conf"
EXPECTED = {
    "permitrootlogin": "no", "passwordauthentication": "no",
    "kbdinteractiveauthentication": "no", "pubkeyauthentication": "yes",
    "authenticationmethods": "publickey",
}
ENV = {"PATH": "/usr/sbin:/usr/bin:/sbin:/bin", "LC_ALL": "C"}


def command(args):
    return subprocess.run(args, env=ENV, capture_output=True, text=True,
                          timeout=15, check=True).stdout


def bounded_read(path):
    with path.open("rb") as stream:
        data = stream.read(256 * 1024 + 1)
    if len(data) > 256 * 1024:
        raise ValueError("SSH policy file exceeds review size limit")
    return data


def directives(data):
    return [words for line in data.decode("utf-8").splitlines()
            if (words := shlex.split(line, comments=True))]


def snapshot():
    paths = [MAIN, *sorted(SNIPPETS.glob("*.conf")), CRYPTO, OPTIONS]
    if SNIPPETS / TARGET not in paths:
        raise ValueError("expected existing early local snippet is absent")
    if len(paths) > 64:
        raise ValueError("SSH include graph exceeds this review's scope")
    return {path: bounded_read(path) for path in paths}


def verify_layout(files):
    expected_include = ["Include", str(SNIPPETS / "*.conf")]
    main_includes = [w for w in directives(files[MAIN]) if w[0].lower() == "include"]
    if main_includes != [expected_include]:
        raise ValueError("unreviewed main Include layout; inspect it before preparing a candidate")
    for path, data in files.items():
        if path == OPTIONS:
            continue
        for words in directives(data):
            if words[0].lower() == "match":
                raise ValueError("existing Match policy requires a separate complete review")
            if words[0].lower() == "include" and path != MAIN:
                if path.parent != SNIPPETS or words[1:] != [str(CRYPTO)]:
                    raise ValueError("unreviewed nested SSH Include")
    options = [line.strip() for line in files[OPTIONS].decode().splitlines()
               if line.strip() and not line.lstrip().startswith("#")]
    if options != ['OPTIONS=""']:
        raise ValueError("unreviewed sshd service environment/options")
    start = command(["systemctl", "show", "sshd.service", "-p", "ExecStart", "--value"])
    match = re.search(r"argv\[\]=([^;]+);", start)
    if not match or match[1].strip() != "/usr/sbin/sshd -D $OPTIONS":
        raise ValueError("unreviewed sshd service invocation")
    environment = command(["systemctl", "show", "sshd.service", "-p", "EnvironmentFiles", "--value"])
    if environment.strip() != f"{OPTIONS} (ignore_errors=yes)":
        raise ValueError("unreviewed sshd service environment files")
    return {"exec_start": "/usr/sbin/sshd -D $OPTIONS", "options_empty": True,
            "environment_file": str(OPTIONS), "existing_match_blocks": False}


def effective(path, context=None):
    args = ["/usr/sbin/sshd", "-T", "-f", str(path)]
    if context:
        args += ["-C", context]
    values = {}
    for line in command(args).splitlines():
        name, _, value = line.partition(" ")
        # Preserve repeated settings when comparing the full effective policy.
        values.setdefault(name, []).append(value)
    return values


def require_policy(values):
    failed = [key for key, value in EXPECTED.items() if values.get(key) != [value]]
    if failed:
        raise ValueError("candidate does not enforce required SSH policy: " + ", ".join(failed))


def render(files, directory, candidate, late=False):
    directory.mkdir()
    includes = directory / "sshd_config.d"
    includes.mkdir()
    for source, data in files.items():
        if source.parent == SNIPPETS:
            (includes / source.name).write_bytes(data)
    (includes / ("99-candidate.conf" if late else TARGET)).write_bytes(candidate)
    old = str(SNIPPETS / "*.conf")
    source = files[MAIN].decode()
    # The unique active Include was checked above. Comments may also mention the
    # path; replacing them cannot change evaluated policy.
    source = source.replace(old, str(includes / "*.conf"))
    main = directory / "sshd_config"
    main.write_text(source)
    return main


def review(candidate_path):
    if os.geteuid() != 0:
        raise ValueError("host policy review requires existing root read/test privileges")
    os.umask(0o077)
    candidate = bounded_read(candidate_path)
    # The candidate is a global snippet, never a new include/Match/key authority.
    if any(w[0].lower() not in {*EXPECTED, "permitemptypasswords", "x11forwarding",
                              "maxauthtries", "logingracetime"} for w in directives(candidate)):
        raise ValueError("candidate contains directives outside the reviewed scope")
    files = snapshot()
    service = verify_layout(files)
    baseline = effective(MAIN)
    contexts = [f"user={user},host=review.invalid,addr={address},laddr=127.0.0.1,lport=22"
                for user in ["linuxuser", "root", "lightning-goats"]
                for address in ["192.0.2.10", "2001:db8::10", "10.8.0.10"]]
    with tempfile.TemporaryDirectory(prefix="lg-sshd-review-", dir="/var/tmp") as temporary:
        root = Path(temporary)
        main = render(files, root / "candidate", candidate)
        command(["/usr/sbin/sshd", "-t", "-f", str(main)])
        proposed = effective(main)
        require_policy(proposed)
        changed = sorted(k for k in baseline.keys() | proposed.keys()
                         if baseline.get(k) != proposed.get(k))
        if not set(changed) <= EXPECTED.keys():
            raise ValueError("candidate changes additional effective settings: " + ", ".join(changed))
        for context in contexts:
            require_policy(effective(main, context))
        # Actual sshd regressions: a late snippet must not look accepted, and a
        # per-user Match relaxation must not be hidden by the global -T result.
        late = render(files, root / "late", candidate, late=True)
        try:
            require_policy(effective(late))
        except ValueError:
            pass
        else:
            raise ValueError("late-include negative control did not reproduce the ordering risk")
        match_file = main.with_name("sshd_config_match")
        match_file.write_text(main.read_text() + "\nMatch User linuxuser\n"
                              "    PasswordAuthentication yes\n    AuthenticationMethods any\n")
        try:
            require_policy(effective(match_file, contexts[0]))
        except ValueError:
            pass
        else:
            raise ValueError("Match negative control did not detect relaxed user policy")
    if snapshot() != files or effective(MAIN) != baseline or verify_layout(files) != service:
        raise ValueError("on-disk policy changed during review; discard this candidate evidence")
    return {"observed_at_utc": datetime.now(timezone.utc).isoformat(),
            "scope": "unapplied Fedora SSH candidate; syntax/effective-policy checks only",
            "reviewer_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            "candidate_sha256": hashlib.sha256(candidate).hexdigest(),
            "installation_target": str(SNIPPETS / TARGET),
            "source_files_sha256": {str(p): hashlib.sha256(b).hexdigest() for p, b in files.items()},
            "service": service,
            "baseline_authentication": {k: baseline.get(k) for k in EXPECTED},
            "candidate_authentication": {k: proposed.get(k) for k in EXPECTED},
            "changed_effective_settings": changed,
            "connection_contexts_checked": contexts,
            "late_include_negative_detected": True, "match_override_negative_detected": True,
            "on_disk_policy_unchanged": True,
            "effective_policy_source": "sshd -T on-disk configuration, not running-listener memory or authentication",
            "applied": False, "service_reloaded": False,
            "operator_login_verified": False, "console_recovery_verified": False,
            "production": "HOLD"}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("candidate", type=Path)
    args = parser.parse_args()
    print(json.dumps(review(args.candidate), indent=2))
