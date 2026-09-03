use std::env;
use std::path::PathBuf;

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

    // Get the absolute path to the project directory
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let ghostty_lib_path = format!("{}/ghostty/zig-out/lib", manifest_dir);

    // Static link pre-built libghostty.a (built by scripts/setup-linux.sh)
    // Use absolute path to ensure it's found
    println!("cargo:rustc-link-search=native={}", ghostty_lib_path);
    println!("cargo:rustc-link-lib=static=ghostty");

    // Modern Ghostty archives bundle their third-party static dependencies.
    // Only platform libraries are linked separately here.
    println!("cargo:rustc-link-lib=dylib=GL");
    println!("cargo:rustc-link-lib=dylib=c++");
    println!("cargo:rustc-link-lib=dylib=c++abi");
    println!("cargo:rustc-link-lib=dylib=stdc++");
    println!("cargo:rustc-link-lib=dylib=gcc_s"); // For __gxx_personality_v0
    println!("cargo:rustc-link-lib=dylib=fontconfig");
    println!("cargo:rustc-link-lib=dylib=freetype");
    if release_build == "1" {
        link_static_libxml2();
    } else {
        println!("cargo:rustc-link-lib=dylib=xml2");
    }

    // Try to link the versioned onig library if dev package isn't installed
    if std::process::Command::new("pkg-config")
        .args(["--exists", "oniguruma"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        println!("cargo:rustc-link-lib=dylib=onig");
    } else if std::path::Path::new("/usr/lib/x86_64-linux-gnu/libonig.so.5").exists() {
        // Link to the versioned library file directly
        println!("cargo:rustc-link-arg=/usr/lib/x86_64-linux-gnu/libonig.so.5");
    }

    // glslang is optional - ghostty can work without it
    // We'll skip it for now since it's not installed

    // Use pkg-config for GTK4/GLib system libraries that libghostty.a needs
    // at link time if they are not fully bundled in the static archive.
    // This is a soft best-effort; link errors reveal which ones are needed.
    if std::process::Command::new("pkg-config")
        .args(["--exists", "gtk4"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        // Emit link-search dirs from the .pc file location (handles extracted dev packages).
        // pkg-config --variable=pcfiledir emits the directory containing the .pc file; the
        // sibling directory (../lib or the pkgconfig parent) contains the .so linker stubs.
        for pkg in &["gtk4", "graphene-gobject-1.0"] {
            let pcdir_out = std::process::Command::new("pkg-config")
                .args(["--variable=pcfiledir", pkg])
                .output();
            if let Ok(out) = pcdir_out {
                let pcdir = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !pcdir.is_empty() {
                    // pkgconfig dir is typically .../lib/x86_64-linux-gnu/pkgconfig;
                    // the parent contains the .so symlinks.
                    let libdir = std::path::Path::new(&pcdir)
                        .parent()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if !libdir.is_empty() {
                        println!("cargo:rustc-link-search=native={libdir}");
                    }
                }
            }
        }

        let gtk4_libs = std::process::Command::new("pkg-config")
            .args(["--libs", "gtk4"])
            .output()
            .expect("pkg-config gtk4 failed");
        let flags = String::from_utf8_lossy(&gtk4_libs.stdout);
        for flag in flags.split_whitespace() {
            if let Some(lib) = flag.strip_prefix("-l") {
                println!("cargo:rustc-link-lib=dylib={lib}");
            } else if let Some(path) = flag.strip_prefix("-L") {
                println!("cargo:rustc-link-search=native={path}");
            }
        }
    }

    // Bind directly to the header produced by the selected Ghostty revision so
    // C ABI changes cannot drift behind a copied header in this repository.
    let ghostty_header = format!("{}/ghostty/zig-out/include/ghostty.h", manifest_dir);
    println!("cargo:rerun-if-changed={ghostty_header}");

    let bindings = bindgen::Builder::default()
        .header(&ghostty_header)
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

fn link_static_libxml2() {
    let output = std::process::Command::new("pkg-config")
        .args(["--libs", "--static", "libxml-2.0"])
        .output()
        .expect("pkg-config is required to statically link libxml2 for release builds");
    if !output.status.success() {
        panic!(
            "pkg-config could not resolve static libxml2 dependencies: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let flags = String::from_utf8_lossy(&output.stdout);
    let mut linked_xml2 = false;
    for flag in flags.split_whitespace() {
        if flag == "-lxml2" {
            if !linked_xml2 {
                println!("cargo:rustc-link-lib=static=xml2");
                linked_xml2 = true;
            }
        } else if let Some(lib) = flag.strip_prefix("-l") {
            println!("cargo:rustc-link-lib=dylib={lib}");
        } else if let Some(path) = flag.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={path}");
        } else {
            println!("cargo:rustc-link-arg={flag}");
        }
    }
    assert!(linked_xml2, "pkg-config did not return -lxml2");
}
