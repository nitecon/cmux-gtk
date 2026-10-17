---
name: cmux-browser
description: Open and reuse shared browser pages inside cmux, automate websites, and let the user complete login or MFA in the same page. Use for browser tasks in a cmux terminal with agent-browser installed.
---

# Browser Automation with cmux

cmux renders agent-browser pages as interactive browser surfaces. The agent and user
share the same page, cookies and login state. Use the cmux CLI to create and close
surfaces so the desktop retains ownership of their browser sessions.

## Discover and reuse

```bash
cmux --json capabilities             # browser.available reports backend discovery
cmux browser list                    # ref, uuid, workspace_uuid, url, profile, session
cmux browser snapshot surface:7 --interactive
```

Reuse a surface relevant to the task. Reading or automating a suspended surface
starts its browser lazily. To navigate it, use `cmux browser goto surface:7 <url>`.
Create a new surface when needed:

```bash
cmux browser open https://example.com
cmux browser open https://example.com --workspace <workspace-uuid>
cmux browser open https://example.com --profile /absolute/path/to/dedicated-profile
```

Retain the returned `surface_ref` and `uuid`. `--profile` selects an agent-browser
Chrome profile name or persistent directory and survives cmux restoration. Without
it, cmux creates an isolated ephemeral browser context.

## Interact

```bash
cmux browser get-url surface:7
cmux browser wait surface:7 --load-state complete --timeout-ms 15000
cmux browser snapshot surface:7 --interactive
cmux browser fill surface:7 e1 "search terms"
cmux browser click surface:7 e2
cmux browser snapshot surface:7 --interactive
```

Use fresh refs after navigation, page changes, or user interaction. Browser commands
return JSON by default; `--no-json` requests human-readable output. Consult
`cmux browser --help` and `cmux browser <command> --help` for the installed CLI's
arguments. Commands follow `cmux browser <command> <surface> ...`.

## Shared login

Opening and automation preserve terminal focus. Use `cmux focus-surface <uuid>`
when the user needs to type, use a password manager, or complete MFA. Let the user
finish, then take a fresh snapshot. Avoid reading password values or vault contents.

For persistent logins, choose a dedicated profile at creation. Extension loading
uses agent-browser's configuration or `AGENT_BROWSER_EXTENSIONS` in the desktop's
environment. cmux displays page content, including extension-injected page controls;
it does not display Chrome's native toolbar or extension toolbar popups.

## Additional agent-browser features

`cmux browser list` exposes the live `session` (null while suspended). Use
`agent-browser --session <session> <command>` for features outside the cmux wrapper,
with the same agent-browser socket directory and namespace as the desktop. Load
`agent-browser skills get core` before using its CLI. Target the session explicitly
to keep user interaction and automation in the same browser.

Close through `cmux browser close --surface surface:7`. An unqualified close closes
all browser surfaces, so specify the intended surface.
