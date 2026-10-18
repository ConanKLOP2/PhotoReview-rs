use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("crate source directory is readable") {
        let path = entry.expect("source entry is readable").path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

#[test]
// C# architecture twin: RecycleBinContractTests.EveryProductionRecycleBin_DeclaresTheSafetyMembersItself
fn every_platform_production_recycle_bin_declares_safety_methods_directly() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&source_root, &mut files);

    let mut implementations = Vec::new();
    let required = ["fn can_recycle(", "fn fits_in_recycle_bin(", "fn delete_permanently("];
    for path in files {
        let source = fs::read_to_string(&path).expect("Rust source is UTF-8");
        if source.contains("impl RecycleBin for ") {
            let missing: Vec<_> = required
                .iter()
                .copied()
                .filter(|method| !source.contains(method))
                .collect();
            assert!(
                missing.is_empty(),
                "{} implements RecycleBin but does not declare {missing:?}",
                path.display()
            );
            implementations.push(path);
        }
    }

    assert!(
        !implementations.is_empty(),
        "architecture scan must discover at least one production RecycleBin implementation"
    );
}
