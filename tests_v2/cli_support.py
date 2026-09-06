"""CLI discovery shared by retained protocol scenarios; these scenarios still need API migration."""
import os
from pathlib import Path

from cmux import cmuxError


def find_cli_binary() -> str:
    """Resolve an executable override or Linux build binary without scanning unrelated build trees.

    CMUXTERM_CLI preserves the existing explicit test override. Otherwise CMUX_BIN_DIR
    selects a build directory, defaulting to this checkout's target/debug. Invalid explicit
    configuration fails immediately so a scenario cannot accidentally run another installation.
    """
    override = os.environ.get("CMUXTERM_CLI")
    build_dir = os.environ.get("CMUX_BIN_DIR")
    candidate = Path(override).expanduser() if override else (
        Path(build_dir).expanduser() if build_dir else Path(__file__).resolve().parents[1] / "target/debug"
    ) / "cmux"
    if not candidate.is_file() or not os.access(candidate, os.X_OK):
        raise cmuxError(f"No executable cmux CLI at {candidate}; build the workspace or set CMUXTERM_CLI / CMUX_BIN_DIR")
    return str(candidate)
