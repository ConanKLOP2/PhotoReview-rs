//! Lane fix-readonly-delete (ST-INVMAP M2, user decision Q2): the production permanent-delete seam must refuse a
//! read-only file like C# `File.Delete` (UnauthorizedAccessException) instead of deleting it like `std::fs::remove_file`.
//! Real-disk but confined to a `TempRoot`; no Recycle Bin is touched.
#[allow(dead_code)]
#[path = "../src/recycle_bin.rs"]
mod recycle_bin;
#[allow(dead_code)]
#[path = "../src/recycle_bin_capacity.rs"]
mod recycle_bin_capacity;
#[allow(dead_code)]
#[path = "../src/recycle_bin_settings.rs"]
mod recycle_bin_settings;
#[allow(dead_code)]
#[path = "../src/recycle_eligibility.rs"]
mod recycle_eligibility;

use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;

use pr_core::abstractions::{NullLog, RecycleBin};
use pr_core::time::DateTimeUtc;
use pr_test_support::TempRoot;
use recycle_bin::RecycleBinRestoreBackend;
use recycle_bin_settings::{DriveType, WindowsRecycleBinSettingsSource};

struct NoRestore;

impl RecycleBinRestoreBackend for NoRestore {
    fn try_restore(&self, _: &Path, _: u64, _: DateTimeUtc) -> io::Result<bool> {
        Ok(false)
    }
}

fn set_read_only(path: &Path, read_only: bool) {
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_readonly(read_only);
    fs::set_permissions(path, permissions).unwrap();
}

/// Facade whose only real seams are the production delete and `exists`; the volume is pretended to be a no-bin drive.
fn bin_with_production_delete() -> recycle_bin::WindowsRecycleBin {
    recycle_bin::WindowsRecycleBin::with_adapters(
        Arc::new(WindowsRecycleBinSettingsSource::default()),
        Arc::new(NullLog),
        |_| Some(DriveType::Removable),
        |_| Some("X:\\".to_owned()),
        |path| path.exists(),
        |_| Ok(0),
        |_| panic!("permanent delete is not a recycle"),
        recycle_bin::native_file_delete,
        Arc::new(NoRestore),
    )
}

#[test]
fn permanent_delete_refuses_a_read_only_file_and_leaves_it_intact() {
    // C# twin: File.Delete on a read-only file throws UnauthorizedAccessException.
    let root = TempRoot::new("ro-delete");
    let file = root.join("protected.jpg");
    fs::write(&file, b"keep me").unwrap();
    set_read_only(&file, true);

    let result = bin_with_production_delete().delete_permanently(&file);

    let still_there = file.exists();
    let content = fs::read(&file).ok();
    if still_there {
        set_read_only(&file, false); // cleanup before TempRoot drop
    }
    let error = result.expect_err("read-only file must not be deleted");
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(still_there, "file must survive the refused delete");
    assert_eq!(content.as_deref(), Some(&b"keep me"[..]));
}

#[test]
fn permanent_delete_removes_a_normal_file_and_accepts_a_missing_one() {
    let root = TempRoot::new("ro-delete-normal");
    let file = root.join("plain.jpg");
    fs::write(&file, b"x").unwrap();
    let bin = bin_with_production_delete();

    bin.delete_permanently(&file).unwrap();
    assert!(!file.exists());
    // C# File.Delete on a missing path succeeds.
    bin.delete_permanently(&file).unwrap();
}
