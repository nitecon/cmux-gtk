"""Measure paced ConPTY output against native GTK drawing and driver diagnostics."""
import json
import subprocess
import sys
import time
from pathlib import Path


def verify(rpc, submit_native, profile, output):
    """Own a disposable workspace and report drawing throughput without assuming hardware speed."""
    fixture = Path(profile) / "render-output.py"
    fixture.write_text(
        "import sys, time\n"
        "for frame in range(180):\n"
        "    sys.stdout.write('\\rCMUX_RENDER_FRAME %04d ' % frame + 'x' * 60)\n"
        "    sys.stdout.flush()\n"
        "    time.sleep(1 / 30)\n"
        "print('\\nCMUX_RENDER_COMPLETE', flush=True)\n"
    )
    workspace = rpc("workspace.create", {"name": "Rendering benchmark", "working_directory": str(profile)})
    try:
        rpc("workspace.select", {"id": workspace["uuid"]})
        surface = next(row["uuid"] for row in rpc("surface.list")["surfaces"]
                       if row["workspace_uuid"] == workspace["uuid"])
        deadline = time.monotonic() + 15
        while True:
            try:
                if rpc("surface.read_text", {"id": surface})["text"].strip():
                    break
            except RuntimeError:
                pass  # Native initialization follows workspace allocation.
            if time.monotonic() > deadline:
                raise RuntimeError("Rendering benchmark shell did not initialize")
            time.sleep(0.1)
        before = rpc("system.diagnostics")
        started = time.monotonic()
        rpc("surface.send_text", {"id": surface, "text": subprocess.list2cmdline([sys.executable, str(fixture)])})
        submit_native()
        deadline = started + 20
        while "CMUX_RENDER_COMPLETE" not in rpc("surface.read_text", {"id": surface})["text"]:
            if time.monotonic() > deadline:
                raise RuntimeError("Paced ConPTY rendering fixture did not complete")
            time.sleep(0.1)
        elapsed = time.monotonic() - started
        after = rpc("system.diagnostics")
        old, new = before["terminal_rendering"], after["terminal_rendering"]
        draws = new["draws"] - old["draws"]
        report = {
            "workload": "180 carriage-return updates paced at 30 Hz through real ConPTY",
            "elapsed_seconds": elapsed, "draws": draws, "draws_per_second": draws / elapsed,
            "requests": new["requests"] - old["requests"],
            "draw_mean_us": (new["draw_total_us"] - old["draw_total_us"]) / max(draws, 1),
            "process_draw_max_us": new["draw_max_us"],
            "opengl": after["first_opengl_context"], "gtk_version": after["gtk_version"],
            "gtk_event_loop": after["gtk_event_loop"],
            "software_override": after["libgl_software_override"],
            "measurement": "GTK draw callbacks; excludes desktop presentation and GPU completion",
        }
        Path(output).write_text(json.dumps(report, indent=2) + "\n")
        print("Windows rendering benchmark:", json.dumps(report), flush=True)
        assert draws > 0, "Paced terminal output completed without a GTK draw"
    finally:
        rpc("workspace.close", {"id": workspace["uuid"]})
