#!/usr/bin/env python3
"""Select exactly the requested crate's native library test from fresh Cargo JSON."""
import argparse
import json
from pathlib import Path
import shutil


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifacts", type=Path)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--target-dir", type=Path)
    args = parser.parse_args()
    manifest = args.manifest.resolve(strict=True)
    target = (args.target_dir or manifest.parent / "target").resolve(strict=True)
    if args.artifacts.stat().st_size > 4 * 1024 * 1024:
        raise ValueError("Cargo artifact stream exceeds bound")
    binaries = set()
    finished = False
    for line in args.artifacts.read_text().splitlines():
        row = json.loads(line)
        if row.get("reason") == "build-finished":
            finished = row.get("success") is True
        if row.get("reason") != "compiler-artifact" or not row.get("profile", {}).get("test") or row.get("target", {}).get("kind") != ["lib"] or not row.get("executable"):
            continue
        if Path(row["manifest_path"]).resolve() != manifest:
            continue
        binary = Path(row["executable"]).resolve(strict=True)
        if not binary.is_relative_to(target) or not binary.is_file():
            raise ValueError("native artifact is outside the selected target directory")
        binaries.add(binary)
    if not finished or len(binaries) != 1:
        raise ValueError("require one native test artifact from a successful build")
    with binaries.pop().open("rb") as source, args.output.open("xb") as destination:
        shutil.copyfileobj(source, destination)


if __name__ == "__main__":
    main()
