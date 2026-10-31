//! Port of PhotoReview platform recycle settings: pure final-path normalization, fakes only.
#![cfg(windows)]

use pr_platform_win::recycle_bin_settings::resolve_final_path_with;
use std::io;
use std::path::PathBuf;

// C#: mount-point lookup follows containing folders; FIX-RECYCLE-C1 adds the fail-closed seam.
// recycle_bin_settings.rs:258 body replacements and :269 NotFound guard mutants in resolve_final_path_with.
#[test]
fn final_path_rebuilds_missing_components_in_order_and_preserves_leaf() {
    let mut asked = Vec::new();
    let result = resolve_final_path_with(r"Q:\source\missing\deeper\photo.jpg", |folder| {
        asked.push(folder.to_path_buf());
        if folder == std::path::Path::new(r"Q:\source") {
            Ok(PathBuf::from(r"\\?\R:\target"))
        } else {
            Err(io::ErrorKind::NotFound.into())
        }
    });
    assert_eq!(
        asked,
        [
            PathBuf::from(r"Q:\source\missing\deeper"),
            PathBuf::from(r"Q:\source\missing"),
            PathBuf::from(r"Q:\source")
        ]
    );
    assert_eq!(result.as_deref(), Some(r"R:\target\missing\deeper\photo.jpg"));
}

// recycle_bin_settings.rs:258 body replacements and :269 NotFound guard; no real drive is queried.
#[test]
fn final_path_roots_and_relative_leaves_do_not_query_and_errors_fail_closed() {
    for input in [r"Q:\", "photo.jpg"] {
        assert_eq!(
            resolve_final_path_with(input, |_| panic!("no containing folder")),
            Some(input.to_owned())
        );
    }
    for kind in [
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::InvalidData,
        io::ErrorKind::NotFound,
    ] {
        let mut calls = 0;
        assert_eq!(
            resolve_final_path_with(r"Q:\source\photo.jpg", |_| {
                calls += 1;
                Err(kind.into())
            }),
            None
        );
        if kind != io::ErrorKind::NotFound {
            assert_eq!(calls, 1, "non-missing failures stop immediately");
        }
    }
}

// recycle_bin_settings.rs:286 guard true/false and == -> != for the drive-prefix colon.
#[test]
fn final_path_normalizes_only_drive_and_unc_extended_prefixes() {
    for (input, canonical, expected) in [
        (
            r"Q:\source\photo.jpg",
            r"\\?\UNC\server\share\target",
            r"\\server\share\target\photo.jpg",
        ),
        (r"Q:\source\photo.jpg", r"\\?\R:\target", r"R:\target\photo.jpg"),
        (r"\\?\Q:\source\photo.jpg", r"\\?\R:\target", r"\\?\R:\target\photo.jpg"),
        (
            r"Q:\source\photo.jpg",
            r"\\?\Volume{fake}\target",
            r"\\?\Volume{fake}\target\photo.jpg",
        ),
        (r"Q:\source\photo.jpg", r"R:\target", r"R:\target\photo.jpg"),
    ] {
        let mut calls = 0;
        let result = resolve_final_path_with(input, |_| {
            calls += 1;
            Ok(PathBuf::from(canonical))
        });
        assert_eq!(result.as_deref(), Some(expected), "{input} -> {canonical}");
        assert_eq!(calls, 1);
    }
}
