//! Build the selected native window bridge without coupling command-line services to GTK.

/// Compile native window integration only when the desktop feature is selected.
fn main() {
    #[cfg(feature = "gtk")]
    build_window_bridge();
}

/// Resolve native headers and emit the selected bridge and its linker dependency.
///
/// Fails the build if pkg-config or required development headers are missing.
#[cfg(feature = "gtk")]
fn build_window_bridge() {
    let (source, packages, library) = match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("windows") => ("native/windows/window_position.c", &["gtk4"][..], "user32"),
        Ok("linux") => (
            "native/linux/window_position.c",
            &["gtk4-x11", "x11"][..],
            "X11",
        ),
        other => panic!("unsupported cmux platform target OS: {other:?}"),
    };
    println!("cargo:rerun-if-changed={source}");
    let flags = std::process::Command::new("pkg-config")
        .arg("--cflags")
        .args(packages)
        .output()
        .expect("pkg-config is required for the GTK platform feature");
    assert!(
        flags.status.success(),
        "GTK4 native development headers are required"
    );
    let mut bridge = cc::Build::new();
    bridge.file(source).flag_if_supported("-Wall");
    for flag in String::from_utf8_lossy(&flags.stdout).split_whitespace() {
        bridge.flag(flag);
    }
    bridge.compile("cmux_window_position");
    println!("cargo:rustc-link-lib={library}");
}
