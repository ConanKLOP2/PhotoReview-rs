//! W9-RENAME-MUTATION-3 killers for cargo-mutants survivors in `catalog/rename.rs`. Each test names its mutant.
use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::catalog::types::CatalogEntry;
use pr_core::file_actions::rename::PathKeyMap;
use pr_core::model::RawPairMode;
use pr_test_support::TempRoot;

fn p(root: &TempRoot, name: &str) -> String {
    root.join(name).to_string_lossy().into_owned()
}

fn paired_catalog(root: &TempRoot) -> ReviewCatalog {
    let entries = ["a.jpg", "a.cr2"]
        .into_iter()
        .map(|name| CatalogEntry::new(p(root, name)).expect("valid TempRoot path"))
        .collect();
    let sidecars = vec![p(root, "a.xmp")];
    let mut catalog = ReviewCatalog::new();
    catalog.reset(entries, RawPairMode::PreferJpeg, Some(&sidecars));
    catalog
}

// Mutant: catalog/rename.rs:118 `rename_in_entry`, `old.jpeg_path() != new.jpeg_path()` -> `==`. A capture group whose
// members map to their own exact spelling did not change, so the rename is a no-op (false, no version bump). With `==`
// an unchanged jpeg path counted as "group changed" and the identity map was applied and reported.
#[test]
fn rename_entries_identity_map_on_a_capture_group_is_a_no_op() {
    let root = TempRoot::new("killer2-group-noop");
    let mut catalog = paired_catalog(&root);
    let version = catalog.structural_version();
    let before = catalog.entries_snapshot().to_vec();
    let identity: PathKeyMap = ["a.jpg", "a.cr2", "a.xmp"]
        .into_iter()
        .map(|name| (p(&root, name), p(&root, name)))
        .collect();
    assert!(!catalog.rename_entries(&identity));
    assert_eq!(version, catalog.structural_version());
    assert_eq!(before, catalog.entries_snapshot().to_vec());
}
