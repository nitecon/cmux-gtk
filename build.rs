//! Shared build metadata and Ghostty ABI generation with separate native link steps.

#[path = "build_support/linux.rs"]
mod linux;
#[path = "build_support/windows.rs"]
mod windows;

use std::env;
use std::path::{Path, PathBuf};

/// Export build metadata, prepare the private Ghostty archive and generate bindings/link inputs.
/// Missing native build tools or required libraries fail the build with explicit diagnostics.
fn main() {
    let version =
        env::var("CMUX_VERSION").unwrap_or_else(|_| env::var("CARGO_PKG_VERSION").unwrap());
    println!(
        "cargo:rustc-env=CMUX_VERSION={}",
        version.trim_start_matches('v')
    );
    println!("cargo:rerun-if-env-changed=CMUX_VERSION");
    let release_build = env::var("CMUX_RELEASE_BUILD").unwrap_or_else(|_| "0".into());
    println!("cargo:rustc-env=CMUX_RELEASE_BUILD={release_build}");
    println!("cargo:rerun-if-env-changed=CMUX_RELEASE_BUILD");
    // Native CI can compile an isolated updater client against a loopback fixture.
    // Public release builds always use GitHub, regardless of the build environment.
    let update_test_api = if release_build == "1" {
        String::new()
    } else {
        env::var("CMUX_UPDATE_TEST_API").unwrap_or_default()
    };
    println!("cargo:rustc-env=CMUX_UPDATE_TEST_API={update_test_api}");
    println!("cargo:rerun-if-env-changed=CMUX_UPDATE_TEST_API");

    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("linux") => linux::build(&root, &release_build),
        Ok("windows") => windows::build(&root),
        other => panic!("unsupported cmux target OS: {other:?}"),
    }
    generate_ghostty_bindings(&root);
}

/// Generate the common embedded surface ABI from the selected Ghostty build header.
/// Missing or incompatible headers and output failures stop the build.
fn generate_ghostty_bindings(root: &Path) {
    // Bind directly to the header produced by the selected Ghostty revision so
    // C ABI changes cannot drift behind a copied header in this repository.
    let ghostty_header = root.join("ghostty/zig-out/include/ghostty.h");
    println!("cargo:rerun-if-changed={}", ghostty_header.display());

    let bindings = bindgen::Builder::default()
        .header(ghostty_header.to_string_lossy())
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        // Needed for types that reference C integer types
        .allowlist_item("ghostty_.*")
        .allowlist_item("GHOSTTY_.*")
        .generate()
        .expect("Unable to generate ghostty bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("ghostty_sys.rs"))
        .expect("Couldn't write ghostty_sys.rs");
}
