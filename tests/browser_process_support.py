"""Read-only accounting for browser processes rooted in fixture-owned daemon PID files."""
from pathlib import Path

from process_support import linux_child_pids


def identity(pid):
    """Return Linux start ticks and state, tolerating concurrent exit; never authorize signalling."""
    try:
        with Path(f"/proc/{pid}/stat").open() as source:
            text = source.read(8192)
    except (FileNotFoundError, ProcessLookupError):
        return None
    fields = text[text.rindex(")") + 2:].split()
    return int(fields[19]), fields[0]


class BrowserProcesses:
    """Track sampled descendants by PID and start time, including those later reparented on shutdown."""

    def __init__(self, directory):
        """Use only the caller's isolated daemon directory; retain at most 512 process identities."""
        self.directory = directory
        self.observed = set()

    def sample(self):
        """Aggregate precise rollup RSS/PSS for reachable daemons and descendants without double-counting PIDs.

        This is a sequential sample, not an atomic process-tree snapshot. Shared pages can appear in
        multiple RSS values; PSS apportions them. Exits are counted separately; permission errors fail.
        """
        roots = set()
        for path in self.directory.glob("*.pid"):
            try:
                with path.open() as source:
                    value = source.read(32).strip()
            except FileNotFoundError:
                continue
            if not value.isdecimal() or int(value) <= 1:
                raise AssertionError("invalid fixture daemon PID")
            roots.add(int(value))
        pending = list(roots)
        seen = set()
        rows = []
        exited = 0
        while pending:
            pid = pending.pop()
            if pid in seen:
                continue
            seen.add(pid)
            if len(seen) > 512:
                raise AssertionError("browser process sample exceeded 512 processes")
            before = identity(pid)
            if before is None or before[1] == "Z":
                exited += 1
                continue
            self.observed.add((pid, before[0]))
            if len(self.observed) > 512:
                raise AssertionError("browser lifetime exceeded 512 observed identities")
            pending.extend(int(child) for child in linux_child_pids(pid))
            try:
                with Path(f"/proc/{pid}/smaps_rollup").open() as source:
                    fields = source.read(65536).splitlines()
            except (FileNotFoundError, ProcessLookupError):
                exited += 1
                continue
            after = identity(pid)
            if after is None or after[0] != before[0] or after[1] == "Z":
                exited += 1
                continue
            values = {line.split(":", 1)[0]: int(line.split()[1]) * 1024
                      for line in fields if line.startswith(("Rss:", "Pss:", "Private_Clean:", "Private_Dirty:"))}
            rows.append({"pid": pid, "start_ticks": before[0], "daemon": pid in roots,
                         "rss_bytes": values["Rss"], "pss_bytes": values["Pss"],
                         "private_bytes": values["Private_Clean"] + values["Private_Dirty"]})
        return {"daemon_count": sum(row["daemon"] for row in rows), "process_count": len(rows),
                "exited_during_sample": exited, "processes": rows,
                **{key: sum(row[key] for row in rows) for key in ("rss_bytes", "pss_bytes", "private_bytes")}}

    def live_observed(self):
        """List still-live sampled identities after close, even if daemon PID files have disappeared."""
        return [pid for pid, ticks in self.observed
                if (current := identity(pid)) is not None and current[0] == ticks and current[1] != "Z"]
