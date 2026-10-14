//! Windows Ghostty import-library preparation and DLL linking.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

/// Prepare and link the native Windows Ghostty DLL import library.
/// Windows DLL symbol resolution does not require Linux's bundled-library symbol namespacing.
pub(super) fn build(root: &Path) {
    let library = root.join("ghostty/zig-out/lib/ghostty-internal.lib");
    let output = PathBuf::from(env::var("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed={}", library.display());
    fs::copy(&library, output.join("libghostty.dll.a"))
        .expect("Build Windows Ghostty first with scripts/setup-windows.sh");
    println!("cargo:rustc-link-search=native={}", output.display());
    // Zig owns the DLL's bundled C++ runtime and dependencies; keep its generated import library intact.
    println!("cargo:rustc-link-lib=dylib=ghostty");
}
