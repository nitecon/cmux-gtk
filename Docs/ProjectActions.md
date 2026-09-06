# Project configuration and actions

The active parity goal includes project actions, workspace definitions, command execution, palette discovery and configurable UI actions. The first implementation provides read-only resolution through `cmux project-actions --directory PATH` (JSON output, no running app required). It does not execute, approve or register commands yet.

The resolver reads global `$XDG_CONFIG_HOME/cmux/cmux.json` (falling back to `$HOME/.config/cmux/cmux.json`) and searches upward from the canonical requested directory for the nearest project file. At each directory `.cmux/cmux.json` wins over `cmux.json`. The winning project action replaces the entire global entry with the same ID; unrelated global actions remain. Each returned action retains its complete raw definition and winning source path.

This matches `findCmuxConfig`, `loadAll`, and `mergedActionEntries` in the inspected upstream `Sources/CmuxConfig.swift` at e36b8e8632a414e2982185f8dae4002a98be2b53. That implementation uses nearest-project action resolution plus global fallback; its separate notification-hook lookup walks the ancestor hierarchy. The directory-actions dogfood README describes broader ancestor inheritance, so do not infer action merging from that README alone.

Reads require regular files, follow symlinks and use the platform nonblocking-open helper to avoid waiting on FIFO peers. Each file is limited to 256 KiB; lookup visits at most 64 directory ancestors; the final registry contains at most 256 actions with nonempty IDs bounded to 128 bytes. Invalid selected files produce errors instead of silently running a lower-precedence file. JSON structure is validated, but action-type-specific schema validation is still outstanding. Other configuration sections are not yet applied.

Next steps are typed command/builtin/agent/workspace/workspaceCommand definitions and explicit target semantics, shared GTK-worker/RPC resolution, reviewable trust for project-defined execution, palette integration, action execution and launch-layout support. Existing launch/resume ownership should be reused. Merely entering a directory or listing actions must never execute project commands. Remote project files require remote reads through their workspace transport; local filesystem resolution is not a substitute.

Actions coverage includes actual directory precedence and source provenance. Strict workspace Clippy passes; runtime execution remains in GitHub Actions. Generated command completions and the man page follow the shared CLI schema.
