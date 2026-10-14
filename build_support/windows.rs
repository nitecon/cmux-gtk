//! Windows Ghostty import-library preparation and DLL linking.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
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
    embed_app_icon(root, &output);
}

/// Wrap the existing PNG artwork in an ICO resource owned by the GUI executable.
/// windres produces the COFF object; no runtime icon handle or extra dependency is required.
fn embed_app_icon(root: &Path, output: &Path) {
    let sizes = [48u32, 128, 256];
    let images: Vec<Vec<u8>> = sizes
        .iter()
        .map(|size| {
            let path = root.join(format!(
                "packaging/icons/hicolor/{size}x{size}/apps/io.cmux.App.png"
            ));
            println!("cargo:rerun-if-changed={}", path.display());
            fs::read(path).expect("read CX icon artwork")
        })
        .collect();
    let mut ico = vec![0, 0, 1, 0, sizes.len() as u8, 0];
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (size, image) in sizes.iter().zip(&images) {
        ico.extend_from_slice(&[*size as u8, *size as u8, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(image.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for image in images {
        ico.extend(image);
    }
    fs::write(output.join("cmux.ico"), ico).expect("write CX ICO resource");
    fs::write(output.join("cmux.rc"), "1 ICON \"cmux.ico\"\n").expect("write icon resource script");
    let status = Command::new("windres")
        .current_dir(output)
        .args(["-i", "cmux.rc", "-o", "cmux-icon.o", "-O", "coff"])
        .status()
        .expect("Windows build requires windres from the MinGW toolchain");
    assert!(status.success(), "compile CX icon resource");
    println!(
        "cargo:rustc-link-arg-bin=cmux-app={}",
        output.join("cmux-icon.o").display()
    );
}
