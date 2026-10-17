//! MSVC Ghostty DLL linking and Windows executable resources.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Link the native MSVC Ghostty DLL through its generated import library.
/// Windows DLL symbol resolution does not require Linux's bundled-library symbol namespacing.
pub(super) fn build(root: &Path) {
    assert_eq!(
        env::var("CARGO_CFG_TARGET_ENV").as_deref(),
        Ok("msvc"),
        "Windows builds require the MSVC Rust toolchain"
    );
    let directory = root.join("ghostty/zig-out/lib");
    let library = directory.join("ghostty-internal.lib");
    let output = PathBuf::from(env::var("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed={}", library.display());
    assert!(
        library.is_file(),
        "Build Ghostty first with scripts/setup-windows.ps1"
    );
    println!("cargo:rustc-link-search=native={}", directory.display());
    println!("cargo:rustc-link-lib=dylib=ghostty-internal");
    embed_app_icon(root, &output);
}

/// Wrap the existing PNG artwork in an ICO resource owned by the Windows entry points.
/// The Windows SDK resource compiler produces the resource consumed by the MSVC linker.
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
    let status = Command::new("rc.exe")
        .current_dir(output)
        .args(["/nologo", "/fo", "cmux.res", "cmux.rc"])
        .status()
        .expect("Windows build requires rc.exe from the Windows SDK");
    assert!(status.success(), "compile CX icon resource");
    println!(
        "cargo:rustc-link-arg-bin=cmux-app={}",
        output.join("cmux.res").display()
    );
    println!(
        "cargo:rustc-link-arg-bin=cmux={}",
        output.join("cmux.res").display()
    );
}
