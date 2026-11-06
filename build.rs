//! Some Linux distributions install the `libxkbcommon-x11` runtime package
//! without the matching `-dev` package. The runtime ships
//! `libxkbcommon-x11.so.0`, but the linker looks for `libxkbcommon-x11.so`.
//! When the unversioned symlink is missing, create one in `OUT_DIR` and point
//! the linker at it so GPUI's X11 backend can link.

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }

    let search_dirs = [
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib/aarch64-linux-gnu",
        "/usr/lib64",
        "/usr/lib",
    ];

    // Nothing to do when the development symlink is already available.
    if search_dirs
        .iter()
        .any(|dir| Path::new(dir).join("libxkbcommon-x11.so").exists())
    {
        return;
    }

    let Some(runtime_lib) = search_dirs
        .iter()
        .map(|dir| Path::new(dir).join("libxkbcommon-x11.so.0"))
        .find(|path| path.exists())
    else {
        return;
    };

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let link = out_dir.join("libxkbcommon-x11.so");
    if !link.exists() {
        #[cfg(unix)]
        std::os::unix::fs::symlink(&runtime_lib, &link).expect("create libxkbcommon-x11 link");
    }
    println!("cargo:rustc-link-search=native={}", out_dir.display());
}
