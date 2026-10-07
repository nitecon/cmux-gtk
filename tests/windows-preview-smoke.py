#!/usr/bin/env python3
"""Exercise the transferred Windows bundle: GUI startup, local RPC and a real ConPTY shell."""
import argparse
import json
import ctypes
from ctypes import wintypes
import os
from pathlib import Path
import subprocess
import shutil
import struct
import tempfile
import time


def capture_terminal(window, destination):
    """Capture the visible client and measure keyboard-updated green background and white glyphs."""
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)
    user32.GetClientRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
    user32.ClientToScreen.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.POINT)]
    user32.GetDC.argtypes = [wintypes.HWND]
    user32.GetDC.restype = wintypes.HDC
    user32.ReleaseDC.argtypes = [wintypes.HWND, wintypes.HDC]
    gdi32.CreateCompatibleDC.argtypes = [wintypes.HDC]
    gdi32.CreateCompatibleDC.restype = wintypes.HDC
    gdi32.CreateDIBSection.argtypes = [wintypes.HDC, ctypes.c_void_p, wintypes.UINT, ctypes.POINTER(ctypes.c_void_p), wintypes.HANDLE, wintypes.DWORD]
    gdi32.CreateDIBSection.restype = wintypes.HANDLE
    gdi32.SelectObject.argtypes = [wintypes.HDC, wintypes.HANDLE]
    gdi32.SelectObject.restype = wintypes.HANDLE
    gdi32.BitBlt.argtypes = [wintypes.HDC, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, wintypes.HDC, ctypes.c_int, ctypes.c_int, wintypes.DWORD]
    gdi32.DeleteObject.argtypes = [wintypes.HANDLE]
    gdi32.DeleteDC.argtypes = [wintypes.HDC]
    rect = wintypes.RECT()
    origin = wintypes.POINT()
    if not user32.GetClientRect(window, ctypes.byref(rect)) or not user32.ClientToScreen(window, ctypes.byref(origin)):
        raise ctypes.WinError(ctypes.get_last_error())
    width, height = rect.right, rect.bottom
    if not (0 < width <= 4096 and 0 < height <= 4096):
        raise RuntimeError(f"Invalid client capture size: {width}x{height}")
    header = struct.pack("<IiiHHIIiiII", 40, width, -height, 1, 32, 0, width * height * 4, 0, 0, 0, 0)
    info = ctypes.create_string_buffer(header)
    pixels = ctypes.c_void_p()
    desktop = user32.GetDC(None)
    memory = gdi32.CreateCompatibleDC(desktop)
    bitmap = gdi32.CreateDIBSection(desktop, info, 0, ctypes.byref(pixels), None, 0)
    previous = None
    try:
        if not desktop or not memory or not bitmap or not pixels.value:
            raise ctypes.WinError(ctypes.get_last_error())
        previous = gdi32.SelectObject(memory, bitmap)
        if not gdi32.BitBlt(memory, 0, 0, width, height, desktop, origin.x, origin.y, 0x00CC0020 | 0x40000000):
            raise ctypes.WinError(ctypes.get_last_error())
        # Finish GDI's bitmap copy before reading the DIB's shared memory on the CPU.
        if not gdi32.GdiFlush():
            raise ctypes.WinError(ctypes.get_last_error())
        raw = ctypes.string_at(pixels, width * height * 4)
    finally:
        if previous:
            gdi32.SelectObject(memory, previous)
        if bitmap:
            gdi32.DeleteObject(bitmap)
        if memory:
            gdi32.DeleteDC(memory)
        if desktop:
            user32.ReleaseDC(None, desktop)
    destination.write_bytes(struct.pack("<2sIHHI", b"BM", 54 + len(raw), 0, 0, 54) + header + raw)

    def is_green(offset):
        """Recognize the shell's new green background without depending on an exact palette."""
        blue, green, red = raw[offset:offset + 3]
        return green >= 40 and green > red + 20 and green > blue + 8

    offsets = [4 * (y * width + x) for y in range(int(height * 0.3), int(height * 0.8), 3) for x in range(int(width * 0.45), int(width * 0.95), 3)]
    green_fraction = sum(is_green(offset) for offset in offsets) / len(offsets)
    glyph_pixels = 0
    for y in range(int(height * 0.12), int(height * 0.45)):
        for x in range(int(width * 0.3), int(width * 0.95)):
            offset = 4 * (y * width + x)
            color = raw[offset:offset + 3]
            if min(color) >= 160 and max(color) - min(color) < 35 and (is_green(offset - 20) or is_green(offset + 20)):
                glyph_pixels += 1
    return {"width": width, "height": height, "green_fraction": green_fraction, "glyph_pixels": glyph_pixels}


def main():
    """Run from a clean profile and check terminal execution before bounded cleanup."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundle", type=Path)
    parser.add_argument("--software-gl-dir", type=Path)
    args = parser.parse_args()
    bundle = args.bundle.resolve()
    for name in ("smoke-result.json", "smoke-terminal.bmp"):
        (bundle / name).unlink(missing_ok=True)
    # Exclude MSYS2 from PATH: the bundle must resolve its own native runtime DLLs.
    env = os.environ.copy()
    env["PATH"] = str(bundle) + os.pathsep + str(Path(os.environ["SystemRoot"]) / "System32")
    env["XDG_DATA_DIRS"] = str(bundle / "share")
    env["CMUX_NO_UPDATE"] = "1"
    env["CMUX_SMOKE"] = "CONPTY_OK"
    for key in ("CMUX_SOCKET", "CMUX_SOCKET_PATH", "HOME", "SHELL"):
        env.pop(key, None)
    with tempfile.TemporaryDirectory(prefix="cmux-windows-smoke-") as profile:
        for key, directory in (("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"), ("XDG_STATE_HOME", "state"), ("XDG_CACHE_HOME", "cache"), ("XDG_RUNTIME_DIR", "runtime")):
            env[key] = str(Path(profile) / directory)
        runtime_bundle = bundle
        if args.software_gl_dir:
            # Overlay driver dependencies only in the disposable CI copy, never the transfer bundle.
            runtime_bundle = Path(profile) / "bundle"
            shutil.copytree(bundle, runtime_bundle)
            for dll in args.software_gl_dir.glob("*.dll"):
                if not (runtime_bundle / dll.name).exists():
                    shutil.copy2(dll, runtime_bundle / dll.name)
            env["PATH"] = str(runtime_bundle) + os.pathsep + str(Path(os.environ["SystemRoot"]) / "System32")
            env["XDG_DATA_DIRS"] = str(runtime_bundle / "share")
            env.update(GALLIUM_DRIVER="llvmpipe", LIBGL_ALWAYS_SOFTWARE="1", CMUX_SMOKE_GRAPHICS="software-opengl")
            env["GDK_DISABLE"] = env.get("GDK_DISABLE", "") + ",egl"
        subprocess.run([str(runtime_bundle / "cmux.exe"), "--version"], env=env, check=True, timeout=20)
        with (bundle / "smoke.log").open("w", encoding="utf-8") as log:
            app = subprocess.Popen([str(runtime_bundle / "cmux-app.exe")], env=env, stdout=log, stderr=subprocess.STDOUT)
            try:
                def rpc(method, params=None):
                    """Call the production CLI and decode its JSON response."""
                    result = subprocess.run([str(runtime_bundle / "cmux.exe"), "--json", "raw", method, "--params", json.dumps(params or {})], env=env, capture_output=True, text=True, timeout=10)
                    if result.returncode:
                        raise RuntimeError(result.stderr)
                    return json.loads(result.stdout)
                deadline = time.monotonic() + 45
                while True:
                    if app.poll() is not None:
                        raise RuntimeError(f"CMUX exited during startup: {app.returncode}")
                    try:
                        ping = rpc("system.ping")
                        break
                    except (RuntimeError, subprocess.TimeoutExpired):
                        if time.monotonic() >= deadline:
                            raise
                        time.sleep(1)
                print("Native GUI/RPC ready:", ping)
                identity = rpc("system.identify")
                if "windows" not in json.dumps(identity):
                    raise RuntimeError(f"Wrong native platform identity: {identity}")
                user32 = ctypes.WinDLL("user32", use_last_error=True)
                user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
                user32.IsWindowVisible.argtypes = [wintypes.HWND]
                callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
                user32.EnumWindows.argtypes = [callback_type, wintypes.LPARAM]
                windows = []
                @callback_type
                def find_window(window, _context):
                    """Find the real visible native application window by its owning PID."""
                    pid = wintypes.DWORD()
                    user32.GetWindowThreadProcessId(window, ctypes.byref(pid))
                    if pid.value == app.pid and user32.IsWindowVisible(window):
                        windows.append(window)
                    return True
                user32.EnumWindows(find_window, 0)
                if not windows:
                    raise RuntimeError("CMUX has no visible native window")
                user32.SetForegroundWindow.argtypes = [wintypes.HWND]
                user32.SetForegroundWindow(windows[0])
                deadline = time.monotonic() + 30
                while True:
                    try:
                        screen = rpc("surface.read_text")
                        if "Microsoft Windows" in json.dumps(screen) or ">" in json.dumps(screen):
                            break
                    except RuntimeError:
                        pass
                    if time.monotonic() >= deadline:
                        raise RuntimeError("Native terminal did not initialize")
                    time.sleep(0.5)
                # The command contains no contiguous result marker, preventing echoed input from passing.
                rpc("surface.send_text", {"text": 'color 1F & cls & echo CMUX_WINDOWS_%CMUX_SMOKE%'})
                # Submit through a real Win32 keyboard event, exercising GTK-to-Ghostty scan-code conversion.
                user32.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
                if not user32.PostMessageW(windows[0], 0x100, 0x0D, 0x001C0001):
                    raise ctypes.WinError(ctypes.get_last_error())
                user32.PostMessageW(windows[0], 0x101, 0x0D, 0xC01C0001)
                deadline = time.monotonic() + 20
                while True:
                    screen = rpc("surface.read_text")
                    if "CMUX_WINDOWS_CONPTY_OK" in json.dumps(screen):
                        break
                    if time.monotonic() >= deadline:
                        raise RuntimeError(f"No real ConPTY shell output: {screen}")
                    time.sleep(0.5)
                print("Real ConPTY shell input/output verified")
                user32.MapVirtualKeyW.argtypes = [wintypes.UINT, wintypes.UINT]
                # Change the color through keyboard input so the pixel check must observe a new frame.
                for character in "color 2f\rcls\recho cmuxnativekeyboard\r":
                    key = 0x0D if character == "\r" else ord(character.upper())
                    scan = user32.MapVirtualKeyW(key, 0)
                    flags = 1 | (scan << 16)
                    for message, detail in ((0x100, flags), (0x101, flags | 0xC0000000)):
                        if not user32.PostMessageW(windows[0], message, key, detail):
                            raise ctypes.WinError(ctypes.get_last_error())
                deadline = time.monotonic() + 20
                while True:
                    screen = rpc("surface.read_text")
                    # One occurrence is the typed command; the second is its real shell output.
                    if json.dumps(screen).count("cmuxnativekeyboard") >= 2:
                        break
                    if time.monotonic() >= deadline:
                        raise RuntimeError(f"Native keyboard text did not execute: {screen}")
                    time.sleep(0.5)
                deadline = time.monotonic() + 15
                while True:
                    frame = capture_terminal(windows[0], bundle / "smoke-terminal.bmp")
                    if frame["green_fraction"] > 0.5 and frame["glyph_pixels"] > 16:
                        break
                    if time.monotonic() >= deadline:
                        raise RuntimeError(f"No visible terminal background/glyphs: {frame}")
                    time.sleep(0.5)
                log.flush()
                messages = (bundle / "smoke.log").read_text(encoding="utf-8", errors="replace")
                if "gdk_gl_context_make_current() failed" in messages or "outside GTK owner thread" in messages:
                    raise RuntimeError("GTK OpenGL ownership failed; see smoke.log")
                print("Visible terminal pixels and native keyboard text verified:", frame)
                (bundle / "smoke-result.json").write_text(json.dumps({"startup": True, "visible_window": True, "local_rpc": True, "conpty_shell": True, "native_enter_key": True, "native_keyboard_text": True, "visible_terminal_pixels": True, "frame": frame, "graphics": env.get("CMUX_SMOKE_GRAPHICS", "system-opengl")}, indent=2) + "\n")
            finally:
                if app.poll() is None:
                    subprocess.run(["taskkill", "/PID", str(app.pid), "/T", "/F"], env=env, capture_output=True, timeout=15)
                app.wait(timeout=15)


if __name__ == "__main__":
    main()
