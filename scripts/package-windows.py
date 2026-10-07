#!/usr/bin/env python3
"""Bundle CMUX and its native DLL dependency closure for transfer to Windows."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess


def main():
    """Resolve native imports, copy runtime data and record the preview revision."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prefix", required=True, type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    bundle = root / "target/windows-preview"
    if bundle.exists():
        shutil.rmtree(bundle)
    bundle.mkdir(parents=True)
    binaries = [root / "target/release" / name for name in ("cmux.exe", "cmux-app.exe")]
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
            dependency = args.prefix / "bin" / name
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
    print(f"Bundled {len(copied)} executables and DLLs at {bundle}")


if __name__ == "__main__":
    main()
