//! Build script of `pr-shell` (unit W8-20a, `contracts/W8-20a.md`): embeds the manifest and the application icon into `PhotoReviewRs.exe` only (never into the test executables or the
//! `r0001-spike` binary), the Rust twin of the C# `ApplicationManifest` / `ApplicationIcon` properties.
//!
//! Steps (MSVC targets only: `CARGO_CFG_TARGET_ENV == "msvc"`; anything else prints nothing and returns):
//! 1. `cargo:rerun-if-changed=` for this file, `build_support/res_writer.rs`, `../pr-platform-win/src/win32/shell.manifest` and the DIRECTORY `../../assets` (not the icon file: a `rerun-if-changed` naming a file that does not exist yet makes cargo re-run the script on every build; a directory is rescanned and a new `PhotoReviewRs.ico` is noticed).
//! 2. `manifest_xml` = the text of `../pr-platform-win/src/win32/shell.manifest` (the one source of truth, tested by the PerMonitorV2 / Common-Controls checks of W8-11c).
//! 3. `ico` = the bytes of `../../assets/PhotoReviewRs.ico` when the file exists; when it does not, `println!("cargo:warning=assets/PhotoReviewRs.ico is missing: the exe is built without an
//!    icon (run tools/sync-from-csharp.ps1)")` and `None` (the build must stay green before the lead syncs the asset; `tests/exe_resources.rs` then fails with a clear message).
//! 4. `res_writer::build_res(&manifest_xml, ico.as_deref())` written to `$OUT_DIR/photoreview.res`; an `Err` is `panic!` with its text (a build script may panic: it fails the build with a message).
//! 5. `println!("cargo:rustc-link-arg-bin=PhotoReviewRs={}", res_path)`. Cargo passes the file to the linker as an input; `link.exe` converts it with `cvtres`. Do NOT use
//!    `cargo:rustc-link-arg` (that also reaches the test executables and `r0001-spike`).
#![allow(dead_code)] // `res_writer` is shared with tests through `#[path]`; the build script uses only `build_res`

#[path = "build_support/res_writer.rs"]
mod res_writer;

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build_support/res_writer.rs");
    println!("cargo:rerun-if-changed=../pr-platform-win/src/win32/shell.manifest");
    println!("cargo:rerun-if-changed=../../assets");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    let manifest_dir =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"));
    let manifest_path = manifest_dir.join("../pr-platform-win/src/win32/shell.manifest");
    let manifest_xml = std::fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", manifest_path.display()));
    let ico_path = manifest_dir.join("../../assets/PhotoReviewRs.ico");
    let ico = match std::fs::read(&ico_path) {
        Ok(bytes) => Some(bytes),
        Err(_) => {
            println!(
                "cargo:warning=assets/PhotoReviewRs.ico is missing: the exe is built without an icon (run tools/sync-from-csharp.ps1)"
            );
            None
        }
    };
    let res = res_writer::build_res(&manifest_xml, ico.as_deref())
        .unwrap_or_else(|e| panic!("cannot build the resource file: {e:?}"));
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let res_path = out_dir.join("photoreview.res");
    std::fs::write(&res_path, res).unwrap_or_else(|e| panic!("cannot write {}: {e}", res_path.display()));
    println!("cargo:rustc-link-arg-bin=PhotoReviewRs={}", res_path.display());
}
