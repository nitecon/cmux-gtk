#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
run_root="$(mktemp -d /tmp/cmux-terminal-close.XXXXXX)"
runtime_dir="$run_root/runtime"
data_dir="$run_root/data"
log_file="$run_root/cmux.log"
mkdir -p "$runtime_dir" "$data_dir"
chmod 700 "$runtime_dir"
socket_path="$runtime_dir/cmux/cmux.sock"

app_pid=""
cleanup() {
    if [[ -n "$app_pid" ]] && kill -0 "$app_pid" 2>/dev/null; then
        kill "$app_pid"
        wait "$app_pid" 2>/dev/null || true
    fi
    rm -rf "$run_root"
}
trap cleanup EXIT
trap 'status=$?; echo "terminal pane close test failed at line $LINENO (status $status)"; cat "$log_file" 2>/dev/null || true; exit "$status"' ERR

GDK_BACKEND=x11 LIBGL_ALWAYS_SOFTWARE=1 \
XDG_RUNTIME_DIR="$runtime_dir" XDG_DATA_HOME="$data_dir" \
    "$repo_root/target/debug/cmux-app" >"$log_file" 2>&1 &
app_pid=$!

for _ in $(seq 1 100); do
    [[ -S "$socket_path" ]] && break
    kill -0 "$app_pid"
    sleep 0.1
done
[[ -S "$socket_path" ]] || { echo "socket was not created"; false; }

cmux=("$repo_root/target/debug/cmux" --socket "$socket_path")
initial_children=0
for _ in $(seq 1 100); do
    initial_children="$(ps --ppid "$app_pid" -o pid= | wc -l)"
    [[ "$initial_children" -ge 1 ]] && break
    sleep 0.1
done
[[ "$initial_children" -ge 1 ]] || { echo "initial terminal child did not start"; false; }

"${cmux[@]}" split --direction horizontal
"${cmux[@]}" split --direction vertical

before_json="$("${cmux[@]}" list-surfaces --json)"
read -r before_count active_uuid < <(python3 -c '
import json, sys
surfaces = json.load(sys.stdin)["surfaces"]
active = next(surface["uuid"] for surface in surfaces if surface["active"])
print(len(surfaces), active)
' <<<"$before_json")
[[ "$before_count" -eq 3 ]] || { echo "expected 3 surfaces, found $before_count"; false; }

before_children="$initial_children"
for _ in $(seq 1 100); do
    before_children="$(ps --ppid "$app_pid" -o pid= | wc -l)"
    [[ "$before_children" -eq $((initial_children + 2)) ]] && break
    sleep 0.1
done
[[ "$before_children" -eq $((initial_children + 2)) ]] || {
    echo "expected $((initial_children + 2)) terminal children, found $before_children"
    false
}
"${cmux[@]}" close-surface "$active_uuid"
"${cmux[@]}" ping >/dev/null

after_json="$("${cmux[@]}" list-surfaces --json)"
after_count="$(python3 -c 'import json,sys; print(len(json.load(sys.stdin)["surfaces"]))' <<<"$after_json")"
[[ "$after_count" -eq 2 ]] || { echo "expected 2 surviving surfaces, found $after_count"; false; }

after_children="$before_children"
for _ in $(seq 1 50); do
    after_children="$(ps --ppid "$app_pid" -o pid= | wc -l)"
    [[ "$after_children" -lt "$before_children" ]] && break
    sleep 0.1
done
[[ "$after_children" -eq $((before_children - 1)) ]] || {
    echo "expected one terminal child to exit ($before_children -> $((before_children - 1))), found $after_children"
    false
}
kill -0 "$app_pid"

if grep -Eq 'gtk_paned_set_(start|end)_child: assertion|segmentation fault|core dumped' "$log_file"; then
    cat "$log_file"
    exit 1
fi

echo "terminal pane close preserved the app and terminated one PTY"
