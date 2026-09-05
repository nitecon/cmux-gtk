//! Build the optional GTK/X11 bridge without coupling command-line services to GTK.

/// Compile native window integration only when the desktop feature is selected.
fn main() {
    #[cfg(feature = "gtk")]
    build_window_bridge();
}

/// Resolve native headers and emit the X11 bridge and its linker dependency.
///
/// Fails the build if pkg-config or required development headers are missing.
#[cfg(feature = "gtk")]
fn build_window_bridge() {
    println!("cargo:rerun-if-changed=native/window_position.c");
    let flags = std::process::Command::new("pkg-config")
        .args(["--cflags", "gtk4-x11", "x11"])
        .output()
        .expect("pkg-config is required for the GTK platform feature");
    assert!(
        flags.status.success(),
        "GTK4 X11 development headers are required"
    );
    let mut bridge = cc::Build::new();
    bridge.file("native/window_position.c");
    for flag in String::from_utf8_lossy(&flags.stdout).split_whitespace() {
        bridge.flag(flag);
    }
    bridge.compile("cmux_window_position");
    println!("cargo:rustc-link-lib=X11");
}
