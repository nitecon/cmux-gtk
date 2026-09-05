//! Configure GDK before it creates the compositor's shared GL context.

/// Apply version-scoped GL workarounds before GTK initialization and worker startup.
pub fn configure() {
    let version = (
        gtk4::major_version(),
        gtk4::minor_version(),
        gtk4::micro_version(),
    );
    append_flags("GDK_DEBUG", &["gl-prefer-gl"]);
    // Match Ghostty's native GTK runtime: its renderer requires desktop GL.
    if version >= (4, 16, 0) {
        append_flags("GDK_DISABLE", &["gles-api", "vulkan"]);
    } else if version >= (4, 14, 0) {
        append_flags("GDK_DEBUG", &["gl-disable-gles", "vulkan-disable"]);
    }
    // These GTK versions can lose the reference to a GL texture's downloaded
    // backing when a dmabuf outlives it. Avoid the affected ownership path.
    // https://github.com/GNOME/gtk/commit/7ff233c7ff2a9949ffd28c9ff55500e1b7578e5e
    // Limit the workaround to the versions audited here; keep desktop GL.
    if ((4, 16, 0)..=(4, 22, 4)).contains(&version) {
        append_flags("GDK_DISABLE", &["dmabuf"]);
    }
    crate::diagnostics::event(format_args!(
        "GTK {}.{}.{} GDK_DEBUG={} GDK_DISABLE={} GSK_RENDERER={}",
        version.0,
        version.1,
        version.2,
        std::env::var("GDK_DEBUG").unwrap_or_default(),
        std::env::var("GDK_DISABLE").unwrap_or_default(),
        std::env::var("GSK_RENDERER").unwrap_or_else(|_| "auto".into()),
    ));
}

/// Add missing GDK flags without replacing explicit user settings or duplicating tokens.
fn append_flags(name: &str, flags: &[&str]) {
    let mut value = std::env::var(name).unwrap_or_default();
    for flag in flags {
        if !value
            .split([':', ' ', ','])
            .any(|existing| existing == *flag)
        {
            if !value.is_empty() {
                value.push(',');
            }
            value.push_str(flag);
        }
    }
    std::env::set_var(name, value);
}
