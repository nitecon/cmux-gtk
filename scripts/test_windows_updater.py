#!/usr/bin/env python3
"""Native Actions acceptance for complete Windows bundle replacement and lock rollback."""
import argparse
import ctypes
import hashlib
import json
import io
import shutil
import subprocess
import tempfile
import threading
import time
import zipfile
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


def digest(path):
    """Hash installed bytes rather than trusting version or metadata alone."""
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    """Run the real staged CLI worker against isolated copies, never the published bundle."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", required=True, type=Path)
    parser.add_argument("--client", required=True, type=Path)
    parser.add_argument("--archive", required=True, type=Path)
    args = parser.parse_args()
    bundle = args.bundle.resolve()
    managed = json.loads((bundle / "managed-files.json").read_text())
    payload = args.archive.read_bytes()
    version = subprocess.check_output([str(bundle / "cmux.exe"), "--version"], text=True).strip().split()[-1]
    state = {"case": "download", "payload": payload}

    class Handler(BaseHTTPRequestHandler):
        """Serve release metadata and checksums to the explicitly fixture-compiled old client."""
        def do_GET(self):
            asset = "cmux-gtk-windows-x86_64.zip"
            base = "http://127.0.0.1:50127"
            if self.path == "/release":
                assets = [{"name": name, "browser_download_url": base + "/" + name}
                          for name in (asset, asset + ".sha256")]
                if state["case"] == "missing-asset":
                    assets = []
                body = json.dumps({"tag_name": "v0.0.1" if state["case"] == "current" else "v" + version,
                                   "assets": assets}).encode()
            elif self.path.endswith(".sha256"):
                digest_value = "0" * 64 if state["case"] == "bad-checksum" else hashlib.sha256(state["payload"]).hexdigest()
                body = (digest_value + "  " + asset + "\n").encode()
            else:
                body = state["payload"]
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    server = ThreadingHTTPServer(("127.0.0.1", 50127), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    with tempfile.TemporaryDirectory(prefix="cmux updater ") as directory:
        root = Path(directory)
        for case in ("complete", "locked", "mapped-image", "invalid-manifest", "rollback"):
            install = root / case
            shutil.copytree(bundle, install)
            (install / "user-notes.txt").write_text("preserve this")
            (install / "session.json").write_text("user session file")
            (install / "obsolete-runtime.dll").write_bytes(b"old bundled runtime")
            (install / "managed-files.json").write_text(json.dumps(managed + ["obsolete-runtime.dll"]))
            staging = install / ".cmux-update-fixture"
            new = staging / "new"
            new.mkdir(parents=True)
            for relative in managed:
                target = new / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(bundle / relative, target)
            (new / "README.txt").write_text("new complete runtime bundle")
            expected = {relative: digest(new / relative) for relative in managed}
            before = {relative: digest(install / relative) for relative in managed}
            handle = None
            mapping = None
            view = None
            kernel = ctypes.WinDLL("kernel32", use_last_error=True)
            kernel.CreateFileW.argtypes = [ctypes.c_wchar_p, ctypes.c_ulong, ctypes.c_ulong,
                                          ctypes.c_void_p, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_void_p]
            kernel.CreateFileW.restype = ctypes.c_void_p
            kernel.CloseHandle.argtypes = [ctypes.c_void_p]
            if case == "locked":
                handle = kernel.CreateFileW(str(install / "ghostty-internal.dll"), 0x80000000, 0, None, 3, 0, None)
                assert handle not in (None, ctypes.c_void_p(-1).value), ctypes.get_last_error()
            if case == "mapped-image":
                kernel.CreateFileMappingW.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_ulong,
                                                      ctypes.c_ulong, ctypes.c_ulong, ctypes.c_wchar_p]
                kernel.CreateFileMappingW.restype = ctypes.c_void_p
                kernel.MapViewOfFile.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_size_t]
                kernel.MapViewOfFile.restype = ctypes.c_void_p
                kernel.UnmapViewOfFile.argtypes = [ctypes.c_void_p]
                image = kernel.CreateFileW(str(install / "cmux-app.exe"), 0x80000000, 7, None, 3, 0, None)
                assert image not in (None, ctypes.c_void_p(-1).value)
                mapping = kernel.CreateFileMappingW(image, None, 0x01000002, 0, 0, None)
                kernel.CloseHandle(image)
                assert mapping, ctypes.get_last_error()
                view = kernel.MapViewOfFile(mapping, 4, 0, 0, 0)
                assert view, ctypes.get_last_error()
            if case == "invalid-manifest":
                (new / "managed-files.json").write_text(json.dumps(managed + ["../outside.txt"]))
            if case == "rollback":
                # A missing late staged file causes failure after replacement has started.
                (new / managed[-1]).unlink()
            try:
                result = subprocess.run([str(new / "cmux.exe"), "__apply-update", str(install), str(staging)],
                                        text=True, capture_output=True, timeout=30)
            finally:
                if handle:
                    kernel.CloseHandle(handle)
                if view:
                    kernel.UnmapViewOfFile(view)
                if mapping:
                    kernel.CloseHandle(mapping)
            print(case, result.returncode, result.stdout, result.stderr)
            assert (result.returncode == 0) == (case == "complete"), result
            assert (install / "user-notes.txt").read_text() == "preserve this"
            assert (install / "session.json").read_text() == "user session file"
            if case == "complete":
                assert not (install / "obsolete-runtime.dll").exists()
                assert all(digest(install / relative) == expected[relative] for relative in managed)
                assert subprocess.check_output([str(install / "cmux.exe"), "--version"], text=True).startswith("cmux ")
            else:
                assert (install / "obsolete-runtime.dll").read_bytes() == b"old bundled runtime"
                assert all(digest(install / relative) == before[relative] for relative in managed)
        for case in ("download", "current", "bad-checksum", "missing-asset", "wrong-platform"):
            state["case"] = case
            state["payload"] = payload
            if case == "wrong-platform":
                output = io.BytesIO()
                with zipfile.ZipFile(io.BytesIO(payload)) as source, zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED) as destination:
                    for name in source.namelist():
                        data = source.read(name)
                        if name == "build.json":
                            build = json.loads(data)
                            build["platform"] = "linux-x86_64"
                            data = json.dumps(build).encode()
                        destination.writestr(name, data)
                state["payload"] = output.getvalue()
            install = root / ("network-" + case)
            shutil.copytree(bundle, install)
            shutil.copy2(args.client, install / "cmux.exe")
            (install / "user-notes.txt").write_text("keep me")
            before = {name: digest(install / name) for name in managed}
            # Exercise both spellings against the no-update response.
            spellings = (["--update"], ["update"]) if case == "current" else (["--update"],)
            for spelling in spellings:
                result = subprocess.run([str(install / "cmux.exe"), *spelling], text=True, capture_output=True, timeout=130)
                print(case, result.returncode, result.stdout, result.stderr)
                assert (result.returncode == 0) == (case in ("download", "current")), result
            if case == "download":
                deadline = time.monotonic() + 30
                log = install / ".cmux-update.log"
                while time.monotonic() < deadline and "Updated CMUX successfully" not in log.read_text(errors="replace"):
                    time.sleep(0.1)
                assert "Updated CMUX successfully" in log.read_text(errors="replace"), log.read_text(errors="replace")
                assert all(digest(install / name) == digest(bundle / name) for name in managed)
            else:
                assert all(digest(install / name) == before[name] for name in managed)
            assert (install / "user-notes.txt").read_text() == "keep me"
    server.shutdown()
    print("Windows full-bundle replacement, preservation, locks, rollback and update aliases passed")


if __name__ == "__main__":
    main()
