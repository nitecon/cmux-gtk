#!/usr/bin/env python3
"""
Regression: zsh prompt redraws should not replay fresh-line OSC 133;A markers.

Prompt themes with async redraws (such as Prezto-like setups) can call
`zle reset-prompt` after the prompt is already visible. Ghostty's zsh shell
integration should emit a single fresh prompt mark for the actual prompt, then
use OSC 133;P for redraws so redraws stay in place instead of looking like
extra prompt lines.
"""

from __future__ import annotations

import os
import shutil
import tempfile
from pathlib import Path

from pty_support import capture_prompt_session


FRESH_PROMPT = b"\x1b]133;A;cl=line\x07"
PROMPT_START = b"\x1b]133;P;k=i\x07"
END_COMMAND = b"\x1b]133;D\x07"
START_OUTPUT = b"\x1b]133;C\x07"


def _write_redrawing_zshrc(path: Path) -> None:
    """Write an isolated zsh prompt that resets once after its asynchronous callback."""
    path.write_text(
        """
autoload -Uz add-zsh-hook

setopt prompt_cr prompt_percent prompt_sp prompt_subst
PROMPT='%F{4}%1~%f %# '
RPROMPT=''

typeset -gi _cmux_redraw_done=0
typeset -g _cmux_redraw_fd=''

# Permit one asynchronous redraw for the next prompt cycle.
_cmux_redraw_precmd() {
  _cmux_redraw_done=0
}

# Unregister and close the readiness pipe, then reset the prompt once.
_cmux_redraw_ready() {
  emulate -L zsh
  local fd="${1:-$_cmux_redraw_fd}"
  if [[ -n "$fd" ]]; then
    zle -F "$fd"
    exec {fd}<&-
  fi
  _cmux_redraw_fd=''
  (( _cmux_redraw_done )) && return 0
  _cmux_redraw_done=1
  zle reset-prompt
}

# Schedule the readiness event when this prompt has not yet redrawn.
_cmux_redraw_line_init() {
  if (( !_cmux_redraw_done )) && [[ -z "$_cmux_redraw_fd" ]]; then
    exec {_cmux_redraw_fd}< <(
      sleep 0.05
      printf 'ready\\n'
    )
    zle -F "$_cmux_redraw_fd" _cmux_redraw_ready
  fi
}

add-zsh-hook precmd _cmux_redraw_precmd
zle -N zle-line-init _cmux_redraw_line_init
""".lstrip(),
        encoding="utf-8",
    )


def main() -> int:
    """Check one fresh prompt marker plus redraw markers in the captured empty-command cycle."""
    root = Path(__file__).resolve().parents[1]
    wrapper_dir = root / "ghostty" / "src" / "shell-integration" / "zsh"
    if not (wrapper_dir / ".zshenv").exists():
        print(f"SKIP: missing Ghostty zsh wrapper at {wrapper_dir}")
        return 0

    zsh_path = shutil.which("zsh")
    if zsh_path is None:
        print("SKIP: zsh not installed")
        return 0

    base = Path(tempfile.mkdtemp(prefix="cmux_ghostty_prompt_redraw_"))
    try:
        home = base / "home"
        home.mkdir(parents=True, exist_ok=True)
        _write_redrawing_zshrc(home / ".zshrc")

        env = dict(os.environ)
        env["HOME"] = str(home)
        env["ZDOTDIR"] = str(wrapper_dir)
        env["GHOSTTY_ZSH_ZDOTDIR"] = str(home)
        env["GHOSTTY_RESOURCES_DIR"] = str(root / "ghostty" / "src")
        env.pop("GHOSTTY_SHELL_FEATURES", None)
        env.pop("GHOSTTY_BIN_DIR", None)

        output = capture_prompt_session([zsh_path, "-d", "-i"], env,
                                        prompt_delay=1.0, exit_delay=2.5, duration=5)

        marker = output.find(END_COMMAND)
        if marker == -1:
            print("FAIL: did not observe OSC 133;D for the empty command prompt cycle")
            return 1

        end = output.find(START_OUTPUT, marker + len(END_COMMAND))
        if end == -1:
            end = len(output)

        prompt_cycle = output[marker:end]
        fresh_count = prompt_cycle.count(FRESH_PROMPT)
        prompt_start_count = prompt_cycle.count(PROMPT_START)

        if fresh_count != 1:
            print(f"FAIL: expected exactly 1 fresh prompt marker after redraw, saw {fresh_count}")
            return 1

        if prompt_start_count < 1:
            print("FAIL: expected redraw path to emit OSC 133;P prompt-start markers")
            return 1

        print("PASS: zsh prompt redraws keep a single fresh prompt marker and reuse OSC 133;P")
        return 0
    finally:
        shutil.rmtree(base, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
