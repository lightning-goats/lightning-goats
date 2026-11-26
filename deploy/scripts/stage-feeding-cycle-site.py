#!/usr/bin/env python3
"""Stage a priced-feeding site update without changing the active site."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import shutil
import tempfile


def single(text, pattern):
    matches = re.findall(pattern, text)
    if len(matches) != 1:
        raise ValueError(f"expected one site reference matching {pattern}")
    return matches[0]


def stage(web, live, output):
    index = (web / "index.html").read_text()
    overlay = (web / "overlay.html").read_text()
    live_index = (live / "index.html").read_text()
    live_overlay = (live / "overlay.html").read_text()
    for stem in ("homepage-payments", "contact-fee"):
        ref = single(live_index, r'src="(/' + stem + r'(?:-[a-f0-9]{12})?\.js)"')
        if not (live / ref[1:]).is_file():
            raise ValueError(f"live site asset is missing: {ref}")
        candidate = single(index, r'src="(/' + stem + r'\.js)"')
        index = index.replace(f'src="{candidate}"', f'src="{ref}"')
    live_payment = single(live_overlay, r'<a id="qr-code" href="(lightning:[a-zA-Z0-9@._-]+)"')
    overlay = re.sub(r'(<a id="qr-code" href=")[^"]+(")', r'\g<1>' + live_payment + r'\2', overlay)

    output.mkdir(mode=0o700)
    for asset, old_ref, text_file in (
        ("feeding-checkout.js", r'(/feeding-checkout\.js\?v=\d+)', "index"),
        ("overlay.js", r'(/overlay\.js\?v=\d+)', "overlay"),
        ("overlay.css", r'(/overlay\.css\?v=\d+)', "overlay"),
    ):
        content = (web / asset).read_bytes()
        suffix = Path(asset).suffix
        name = f"{Path(asset).stem}-{hashlib.sha256(content).hexdigest()[:12]}{suffix}"
        if text_file == "index":
            index = index.replace(single(index, old_ref), "/" + name)
        else:
            overlay = overlay.replace(single(overlay, old_ref), "/" + name)
        (output / name).write_bytes(content)
    (output / "index.html").write_text(index)
    (output / "overlay.html").write_text(overlay)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("source")
    parser.add_argument("sha256")
    parser.add_argument("live_root", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    spec = importlib.util.spec_from_file_location("release", Path(__file__).with_name("smoke-release.py"))
    release = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(release)
    with tempfile.TemporaryDirectory(prefix="lg-priced-site-") as temporary:
        temp = Path(temporary)
        archive = temp / "release.tar.gz"
        shutil.copyfile(args.archive, archive)
        with archive.open("rb") as source:
            if hashlib.file_digest(source, "sha256").hexdigest() != args.sha256:
                raise ValueError("archive hash mismatch")
        payload = temp / "payload"
        payload.mkdir()
        release.verify_archive(archive, args.source, payload)
        stage(payload / "web", args.live_root, args.output)
        shutil.copyfile(payload / "deploy/scripts/update-overlay-data.py", args.output / "update-overlay-data.py")
    print(json.dumps({"source": args.source, "files": sorted(path.name for path in args.output.iterdir())}))


if __name__ == "__main__":
    main()
