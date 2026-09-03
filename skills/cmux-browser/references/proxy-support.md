# Proxy Support

How proxy behavior works for cmux browser automation.

**Related**: [commands.md](commands.md), [SKILL.md](../SKILL.md)

## Contents

- [Current Behavior](#current-behavior)
- [What Is Not Exposed via CLI](#what-is-not-exposed-via-cli)
- [Workarounds](#workarounds)
- [Verification](#verification)

## Current Behavior

cmux launches Chromium through `agent-browser`. Network behavior follows the
environment inherited by that process.

## What Is Not Exposed via CLI

There is currently no first-class `cmux browser proxy ...` command for per-surface proxy routing.

cmux does not currently pass a per-surface proxy configuration to
`agent-browser`.

## Workarounds

1. Configure system/network-level proxy for the environment where cmux runs.
2. Route traffic through an upstream gateway you control.
3. Validate behavior with explicit IP checks.

## Verification

```bash
cmux browser open https://httpbin.org/ip --json
cmux browser surface:7 get text body
```

Compare returned IP against expected proxy egress.
