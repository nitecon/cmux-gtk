# Components

These boundaries guide the ongoing refactor. Locations describe the current implementation; outstanding moves are recorded in [RefactorAudit](RefactorAudit.md).

| Component | Responsibility | Current source |
| --- | --- | --- |
| Desktop composition | Construct GTK application, connect events, own application lifetime | `src/main.rs`, header/menu/dialog modules |
| Workspace model | Workspace identity, launch settings, order and attention | `src/workspace.rs`, `src/app_state.rs` |
| Pane tree | Splits, sibling tabs, selection and surface lifecycle | `src/split_engine.rs` |
| Terminal adapter | Ghostty configuration, input, clipboard and rendering through its C ABI | `src/ghostty` |
| Browser adapter | Optional browser daemon, commands, previews and navigation | `src/browser.rs` |
| Command interface | CLI argument contract, protocol validation and dispatch | `src/cli`, `src/socket` |
| Session/configuration | Serializable state, compatibility, atomic writes | `src/session.rs`, `src/config.rs`, `src/ssh_hosts.rs` |
| Remote transport | SSH deployment, reconnect, stream and PTY ownership | `src/ssh`, `daemon/remote` |
| Linux services | XDG locations, peer credentials and optional GTK/X11 placement; remaining process/notification/native services are pending extraction | `crates/cmux-platform` |
| Distribution | Dependency setup, linking, packages and release publishing | `build.rs`, `scripts`, `packaging`, `.github/workflows` |

The Linux library exposes small typed functions, with no dependency on workspace state. CLI use must not require GTK initialization. GTK-specific services can be an optional feature. Keep Unix transport and Linux kernel details at this boundary as extraction progresses. A future platform provides equivalent services; the current refactor does not implement unsupported platforms.

Share repeated behavior at its owning component. UI and socket callers should invoke the same workspace operations; configuration files should use the same path and atomic-write helpers; credential tests must call the production authentication function. Do not duplicate those rules in adapters or tests.
