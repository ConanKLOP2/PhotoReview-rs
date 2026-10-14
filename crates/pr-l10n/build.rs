//! Build script of `pr-l10n`: generates `keys` (one constant per English catalog key) and `tr` (one typed accessor per
//! key or plural group) from `assets/languages/en.json` (C# `PhotoReview.Localization.Generator`, ADR 0006).
//!
//! The analysis lives in `build/keygen.rs`, shared with `tests/key_generator.rs`. A diagnostic (PRLOC001..007) fails the
//! build: every PRLOC is an error in C# too.

// The tests compile the same file and use more of it than the build script does.
#[allow(dead_code)]
#[path = "build/keygen.rs"]
mod keygen;

use std::error::Error;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build/keygen.rs");
    println!("cargo:rerun-if-changed=../../assets/languages/en.json");

    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("cargo did not set OUT_DIR")?);
    let manifest_dir =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").ok_or("cargo did not set CARGO_MANIFEST_DIR")?);
    let catalog_path = manifest_dir
        .join("..")
        .join("..")
        .join("assets")
        .join("languages")
        .join("en.json");

    let input = keygen::CatalogInput {
        path: catalog_path.to_string_lossy().into_owned(),
        content: std::fs::read_to_string(&catalog_path).ok(),
    };
    let plan = keygen::plan(&[input]);

    // Both files are written on every run so `include!` always finds them (also when the build then fails).
    let (keys, tr) = if plan.generated {
        (keygen::render_keys(&plan), keygen::render_tr(&plan))
    } else {
        (
            "// not generated: see the build error\n".to_owned(),
            "// not generated: see the build error\n".to_owned(),
        )
    };
    std::fs::write(out_dir.join("tr_keys.rs"), keys)?;
    std::fs::write(out_dir.join("tr_fns.rs"), tr)?;

    if !plan.diagnostics.is_empty() {
        let mut lines = Vec::new();
        for d in &plan.diagnostics {
            let line = format!("{}: {}", d.id, d.message);
            println!("cargo:warning={line}");
            lines.push(line);
        }
        return Err(lines.join("\n").into());
    }
    Ok(())
}
