#!/usr/bin/env python3
"""Prepare an encrypted webhook secret and register the Lightning Goats callback.

Default: print settings only. --prepare-secret: provision a missing encrypted
signing secret. --apply: register a DISABLED subscription using a prompted setup
key. --apply --enable: require a live endpoint, then register/enable it.
The setup key needs partner.webhooks.manage and is never saved in the daemon.
"""
import argparse
import getpass
import hashlib
import hmac
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile
import urllib.error
import urllib.request
import uuid

URL = "https://lightning-goats.com/api/v1/strike/webhook"
EVENT = "receive-request.receive-completed"
SECRET_PATH = Path("/etc/credstore.encrypted/lightning-goats-strike-webhook-secret")
DIAGNOSTIC = {"stage": "startup", "http_status": None}


def save_diagnostic(error_type):
    """Store fixed stage names/status codes only; never terminal or response text."""
    if os.geteuid() != 0:
        return
    directory = Path("/run/lightning-goats-setup")
    if directory.is_symlink():
        return
    directory.mkdir(mode=0o700, exist_ok=True)
    metadata = directory.stat()
    if metadata.st_uid != 0 or metadata.st_mode & 0o077:
        return
    with tempfile.NamedTemporaryFile(mode="w", dir=directory, delete=False) as file:
        json.dump({**DIAGNOSTIC, "error_type": error_type}, file)
        temporary = file.name
    os.replace(temporary, directory / "last-webhook-error.json")


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def secret():
    directory = SECRET_PATH.parent
    if directory.is_symlink() or SECRET_PATH.is_symlink():
        raise ValueError("refusing a symlink credential path")
    directory.mkdir(mode=0o700, exist_ok=True)
    if directory.stat().st_uid != 0 or directory.stat().st_mode & 0o022:
        raise ValueError("credential directory must be root-owned and not writable by other users")
    if not SECRET_PATH.exists():
        value = secrets.token_hex(20)  # 40 chars: Strike requires 10–50.
        with tempfile.TemporaryDirectory(prefix=".lg-webhook-", dir=directory) as temporary:
            encrypted = Path(temporary) / "secret"
            subprocess.run(["systemd-creds", "encrypt", "--name=strike-webhook-secret", "-", str(encrypted)],
                           input=value.encode(), check=True, capture_output=True)
            encrypted.chmod(0o600)
            os.link(encrypted, SECRET_PATH)  # Exclusive; never replaces an existing secret.
        subprocess.run(["restorecon", str(SECRET_PATH)], check=True, capture_output=True)
        print("Generated and encrypted the webhook signing secret.")
    metadata = SECRET_PATH.stat()
    if metadata.st_uid != 0 or metadata.st_mode & 0o077:
        raise ValueError("credential must be root-owned and private")
    value = subprocess.run(["systemd-creds", "decrypt", "--name=strike-webhook-secret", str(SECRET_PATH), "-"],
                           check=True, capture_output=True).stdout.decode().strip()
    if not 10 <= len(value) <= 50:
        raise ValueError("the saved webhook secret must contain 10–50 characters")
    return value


def setup_key(path):
    if path is None:
        key = getpass.getpass("Temporary Strike setup key (partner.webhooks.manage): ").strip()
    else:
        if path.is_symlink() or not path.is_file():
            raise ValueError("setup key source must be a regular file, not a symlink")
        metadata = path.stat()
        if metadata.st_uid != 0 or metadata.st_mode & 0o777 != 0o600 or metadata.st_size > 16384:
            raise ValueError("setup key source must be root-owned, mode 0600 and at most 16 KiB")
        key = path.read_text().strip()
    if not key or any(ord(c) < 33 or ord(c) > 126 for c in key):
        raise ValueError("setup key is empty or contains whitespace/control characters")
    return key


def validate_subscription(item):
    if not isinstance(item, dict):
        raise ValueError("unexpected subscription response")
    if item.get("webhookUrl") != URL or item.get("webhookVersion") != "v1" or item.get("eventTypes") != [EVENT]:
        raise ValueError("subscription settings conflict with the selected daemon; nothing further changed")
    try:
        uuid.UUID(str(item.get("id")))
    except ValueError:
        raise ValueError("subscription returned an invalid ID") from None
    if not isinstance(item.get("enabled"), bool):
        raise ValueError("subscription response is missing enabled state")
    return item


def verify_receiver(opener, value):
    subprocess.run(["systemctl", "is-active", "--quiet", "lightning-goats-receive.service"], check=True)
    body = b"{}"  # Signed but incomplete: can never enqueue a completed receive.
    for signature, expected in [(None, 401), (hmac.new(value.encode(), body, hashlib.sha256).hexdigest(), 400)]:
        headers = {"Content-Type": "application/json"}
        if signature is not None:
            headers["X-Webhook-Signature"] = signature
        request = urllib.request.Request(URL, data=body, headers=headers)
        try:
            with opener.open(request, timeout=10) as response:
                status = response.status
        except urllib.error.HTTPError as error:
            status = error.code
            error.close()
        if status != expected:
            raise ValueError("live webhook signature preflight failed; subscription was not enabled")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prepare-secret", action="store_true")
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--enable", action="store_true")
    parser.add_argument("--key-file", type=Path, help="root-owned mode-0600 file containing only the temporary setup API key")
    args = parser.parse_args()
    if args.enable and not args.apply:
        parser.error("--enable requires --apply")
    if args.key_file and not args.apply:
        parser.error("--key-file requires --apply")
    print(json.dumps({"webhookUrl": URL, "webhookVersion": "v1", "eventTypes": [EVENT],
                      "enabled": args.enable, "setup_scope": "partner.webhooks.manage"}, indent=2))
    if not (args.prepare_secret or args.apply):
        print("No changes. Use --prepare-secret or --apply; see --help.")
        return
    if os.geteuid() != 0:
        raise ValueError("run with sudo python3; encrypted credentials must be root-owned")
    DIAGNOSTIC["stage"] = "prepare_secret"
    value = secret()
    if not args.apply:
        print("Secret ready. No Strike API request or service change was made.")
        return
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    if args.enable:
        DIAGNOSTIC["stage"] = "check_live_endpoint"
        verify_receiver(opener, value)
    DIAGNOSTIC["stage"] = "prompt_setup_key"
    key = setup_key(args.key_file)

    def request(method, path, data=None):
        DIAGNOSTIC["stage"] = {"GET": "list_subscriptions", "POST": "create_subscription", "PATCH": "enable_subscription"}[method]
        body = None if data is None else json.dumps(data).encode()
        req = urllib.request.Request("https://api.strike.me/v1/" + path, data=body, method=method,
            headers={"Authorization": "Bearer " + key, "Content-Type": "application/json", "Accept": "application/json"})
        try:
            with opener.open(req, timeout=15) as response:
                expected_status = 201 if method == "POST" else 200
                if response.status != expected_status:
                    raise ValueError("unexpected Strike subscription response status")
                body = response.read(65537)
                if len(body) > 65536:
                    raise ValueError("Strike response exceeded 64 KiB")
                return json.loads(body) if body else None
        except urllib.error.HTTPError as error:
            DIAGNOSTIC["http_status"] = error.code
            error.close()
            if error.code == 401:
                raise ValueError("Strike rejected the setup key (HTTP 401). Check that it is a production API key.") from None
            if error.code == 403:
                raise ValueError("Strike denied webhook management (HTTP 403). Use a separate setup key with partner.webhooks.manage.") from None
            raise ValueError(f"Strike returned HTTP {error.code}; response body withheld") from None
        except (urllib.error.URLError, TimeoutError):
            raise ValueError("Strike request failed; inspect subscriptions before retrying a create") from None

    existing = request("GET", "subscriptions")
    if not isinstance(existing, list):
        raise ValueError("unexpected subscriptions response")
    if any(not isinstance(item, dict) for item in existing):
        raise ValueError("unexpected subscription list entry")
    matches = [item for item in existing if item.get("webhookUrl") == URL]
    if matches:
        if len(matches) != 1 or matches[0].get("webhookVersion") != "v1" or matches[0].get("eventTypes") != [EVENT]:
            raise ValueError("an existing subscription conflicts with these settings; nothing changed")
        item = validate_subscription(matches[0])
        if args.enable:
            # PATCH sends the locally saved secret too, ensuring the two sides agree.
            item = request("PATCH", "subscriptions/" + str(item["id"]), {
                "webhookUrl": URL, "webhookVersion": "v1", "eventTypes": [EVENT], "secret": value, "enabled": True})
        elif item.get("enabled"):
            print("Existing enabled subscription preserved. Its signing secret still needs live verification.")
    else:
        item = request("POST", "subscriptions", {"webhookUrl": URL, "webhookVersion": "v1",
            "eventTypes": [EVENT], "secret": value, "enabled": args.enable})
    item = validate_subscription(item)
    if args.enable and not item["enabled"]:
        raise ValueError("Strike did not enable the subscription")
    print("Subscription ID: " + str(item["id"]))
    print("Subscription is enabled." if item["enabled"] else "Prepared disabled; run again with --apply --enable after deployment.")
    if not args.enable and not matches and item["enabled"]:
        raise ValueError("provider unexpectedly enabled a subscription requested as disabled")
    diagnostic = Path("/run/lightning-goats-setup/last-webhook-error.json")
    if diagnostic.exists():
        diagnostic.unlink()
    print("Revoke the temporary setup key when registration is complete; keep the runtime key receive-only.")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        save_diagnostic(type(error).__name__)
        raise SystemExit(str(error)) from None
