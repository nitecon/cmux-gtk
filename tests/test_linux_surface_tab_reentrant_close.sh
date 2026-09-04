#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
run_root="$(mktemp -d /tmp/cmux-surface-tab-close.XXXXXX)"
runtime_dir="$run_root/runtime"
data_dir="$run_root/data"
state_dir="$run_root/state"
browser_dir="$run_root/browser"
process_log="$run_root/process.log"
diagnostic_log="$run_root/cmux.log"
mock_browser="$run_root/agent-browser"
mkdir -p "$runtime_dir" "$data_dir/cmux" "$state_dir" "$browser_dir"
chmod 700 "$runtime_dir"
cp "$repo_root/tests/fixtures/session_terminal_over_browser.json" "$data_dir/cmux/session.json"
cp "$repo_root/tests/fixtures/mock_agent_browser.py" "$mock_browser"
chmod +x "$mock_browser"
socket_path="$runtime_dir/cmux/cmux.sock"

app_pid=""
cleanup() {
    if [[ -n "$app_pid" ]] && kill -0 "$app_pid" 2>/dev/null; then
        kill "$app_pid"
        wait "$app_pid" 2>/dev/null || true
    fi
    if [[ -f "$browser_dir/mock.pid" ]]; then
        mock_pid="$(<"$browser_dir/mock.pid")"
        kill "$mock_pid" 2>/dev/null || true
    fi
    rm -rf "$run_root"
}
trap cleanup EXIT
trap 'status=$?; echo "surface tab reentrant close test failed at line $LINENO (status $status)"; cat "$process_log" 2>/dev/null || true; exit "$status"' ERR

GDK_BACKEND=x11 LIBGL_ALWAYS_SOFTWARE=1 \
XDG_RUNTIME_DIR="$runtime_dir" XDG_DATA_HOME="$data_dir" XDG_STATE_HOME="$state_dir" \
AGENT_BROWSER_SOCKET_DIR="$browser_dir" CMUX_AGENT_BROWSER="$mock_browser" \
CMUX_LOG="$diagnostic_log" \
    "$repo_root/target/debug/cmux-app" >"$process_log" 2>&1 &
app_pid=$!

for _ in $(seq 1 100); do
    [[ -S "$socket_path" ]] && break
    kill -0 "$app_pid"
    sleep 0.1
done
[[ -S "$socket_path" ]] || { echo "socket was not created"; false; }

cmux=("$repo_root/target/debug/cmux" --socket "$socket_path")
for _ in $(seq 1 100); do
    grep -q "browser tab wiring complete uuid=20000000-0000-4000-8000-000000000002" "$diagnostic_log" && break
    kill -0 "$app_pid"
    sleep 0.1
done
grep -q "browser tab wiring complete uuid=20000000-0000-4000-8000-000000000002" "$diagnostic_log"
"${cmux[@]}" close-surface 30000000-0000-4000-8000-000000000003
"${cmux[@]}" ping >/dev/null

for _ in $(seq 1 50); do
    grep -q "browser map deferred while application state is busy" "$diagnostic_log" && break
    sleep 0.1
done
grep -q "browser map deferred while application state is busy" "$diagnostic_log"
grep -q "surface-tab closed uuid=30000000-0000-4000-8000-000000000003" "$diagnostic_log"
! grep -q "PANIC" "$diagnostic_log"
kill -0 "$app_pid"

echo "terminal tab close deferred reentrant browser mapping and preserved the app"
