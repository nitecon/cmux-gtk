#!/usr/bin/env bash
# Shared validation reporting. Source once per validator; callers decide final exit status.
PASS=0
FAIL=0

# Run the command arguments after a description, suppress output and count its result.
# Always returns success so strict-mode callers can report every independent failure.
check() {
    local description="$1"
    shift
    if "$@" >/dev/null 2>&1; then
        printf '  PASS: %s\n' "$description"
        PASS=$((PASS + 1))
    else
        printf '  FAIL: %s\n' "$description"
        FAIL=$((FAIL + 1))
    fi
}

# Succeed only when grep finds no matching line; preserve read/usage errors.
# Arguments are an extended regular expression and an input file path.
absent() {
    local status=0
    grep -E "$1" "$2" >/dev/null || status=$?
    case "$status" in
        0) return 1 ;;
        1) return 0 ;;
        *) return "$status" ;;
    esac
}
