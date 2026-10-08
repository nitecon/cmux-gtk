#!/usr/bin/env python3
"""Bundle CMUX and its native DLL dependency closure for transfer to Windows."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import zipfile


def main():
    """Resolve native imports and record every bundled file for safe replacement."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prefix", required=True, type=Path)
    parser.add_argument("--archive", type=Path, help="Write a flat portable ZIP and SHA256 manifest")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    bundle = root / "target/windows-preview"
    if bundle.exists():
        shutil.rmtree(bundle)
    bundle.mkdir(parents=True)
    ghostty = root / "ghostty/zig-out/lib/ghostty-internal.dll"
    binaries = [root / "target/release" / name for name in ("cmux.exe", "cmux-app.exe")] + [ghostty]
    pending = list(binaries)
    copied = set()
    system = Path(os.environ["SystemRoot"]) / "System32"
    while pending:
        source = pending.pop()
        if source.name.lower() in copied:
            continue
        copied.add(source.name.lower())
        shutil.copy2(source, bundle / source.name)
        output = subprocess.check_output(["objdump", "-p", str(source)], text=True)
        for name in re.findall(r"DLL Name:\s*(\S+)", output):
            dependency = ghostty if name.lower() == ghostty.name.lower() else args.prefix / "bin" / name
            if dependency.is_file():
                pending.append(dependency)
            elif not (system / name).is_file() and not name.lower().startswith(("api-ms-", "ext-ms-")):
                raise RuntimeError(f"Unresolved runtime import {name} from {source.name}")
    for relative in ("share/glib-2.0/schemas", "share/icons/Adwaita", "share/icons/hicolor", "share/licenses"):
        source = args.prefix / relative
        if source.is_dir():
            shutil.copytree(source, bundle / relative, dirs_exist_ok=True)
    shutil.copy2(root / "LICENSE", bundle / "LICENSE")
    shutil.copy2(root / "Docs/WindowsPreview.md", bundle / "README.txt")
    (bundle / "launch-cmux.cmd").write_text('@echo off\nset "PATH=%~dp0;%PATH%"\nset "XDG_DATA_DIRS=%~dp0share"\nstart "" "%~dp0cmux-app.exe"\n', encoding="utf-8")
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    (bundle / "build.json").write_text(json.dumps({"revision": revision, "platform": "windows-x86_64", "runtime_files": sorted(copied)}, indent=2) + "\n", encoding="utf-8")
    managed = sorted(path.relative_to(bundle).as_posix() for path in bundle.rglob("*") if path.is_file())
    managed = sorted(managed + ["managed-files.json"])
    (bundle / "managed-files.json").write_text(json.dumps(managed, indent=2) + "\n", encoding="utf-8")
    if args.archive:
        args.archive.parent.mkdir(parents=True, exist_ok=True)
        with zipfile.ZipFile(args.archive, "w", zipfile.ZIP_DEFLATED) as archive:
            for relative in managed:
                archive.write(bundle / relative, relative)
        with args.archive.open("rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        args.archive.with_suffix(args.archive.suffix + ".sha256").write_text(f"{digest}  {args.archive.name}\n", encoding="ascii")
    print(f"Bundled {len(copied)} executables and DLLs at {bundle}")


if __name__ == "__main__":
    main()
