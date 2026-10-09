#!/usr/bin/env python3
"""Fetch a pinned official provider package, verifying its published SHA before CI execution."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import tarfile
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--platform", choices=("linux", "windows"), required=True)
    args = parser.parse_args()
    platform = "x86_64-pc-windows-msvc" if args.platform == "windows" else "x86_64-unknown-linux-musl"
    name = f"codex-package-{platform}.tar.gz"
    release = json.load(urllib.request.urlopen("https://api.github.com/repos/openai/codex/releases/tags/rust-v0.161.0", timeout=30))
    asset = next(asset for asset in release["assets"] if asset["name"] == name)
    digest = asset.get("digest", "")
    if not digest.startswith("sha256:"):
        raise RuntimeError("Official provider asset has no SHA256 digest")
    data = urllib.request.urlopen(asset["browser_download_url"], timeout=90).read()
    if hashlib.sha256(data).hexdigest() != digest.removeprefix("sha256:"):
        raise RuntimeError("Official Codex fixture checksum mismatch")
    args.destination.mkdir(parents=True, exist_ok=True)
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        archive.extractall(args.destination, filter="data")
    executable = "codex.exe" if args.platform == "windows" else "codex"
    matches = [path for path in args.destination.rglob(executable) if path.is_file()]
    if len(matches) != 1:
        raise RuntimeError("Provider package must contain exactly one native Codex binary")
    (args.destination / "executable.txt").write_text(str(matches[0].resolve()), encoding="utf-8")
    print("Verified official Codex 0.161 fixture:", name, digest)


if __name__ == "__main__":
    main()
