//! FIX-RECYCLE-C1: the volume asked about for a recycle decision is the one that holds the file after links are
//! resolved. Fakes only (no Shell, no Recycle Bin); the `#[ignore]` test touches only a junction inside a TempRoot and
//! calls the read-only volume query.

use pr_platform_win::recycle_bin_settings::{DriveType, query_mount_point, resolve_final_path_with};
use pr_platform_win::recycle_eligibility::{can_recycle_with, is_permanent_delete_allowed_with};
use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};

/// `C:\link` is a junction/symlink to `E:\real`; everything else under `C:\` is a plain folder.
fn fake_canonicalize(path: &Path) -> io::Result<PathBuf> {
    let text = path.to_string_lossy().into_owned();
    if let Some(rest) = text.strip_prefix(r"C:\link") {
        if rest.is_empty() || rest.starts_with(char::from(92u8)) {
            return if rest.contains(r"\missing") {
                Err(io::Error::from(io::ErrorKind::NotFound))
            } else {
                Ok(PathBuf::from(format!(r"\\?\E:\real{rest}")))
            };
        }
    }
    if text.starts_with(r"C:\loop") {
        return Err(io::Error::from_raw_os_error(1921));
    }
    Ok(PathBuf::from(format!(r"\\?\{text}")))
}

fn drive_of(root: &str) -> Option<DriveType> {
    match root.chars().next() {
        Some('C') => Some(DriveType::Fixed),
        Some('E') => Some(DriveType::Removable),
        _ => None,
    }
}

fn mount_of(path: &str) -> Option<String> {
    path.get(..3).map(str::to_owned)
}

#[test]
fn folder_link_to_another_volume_resolves_to_the_target_volume() {
    let resolved = resolve_final_path_with(r"C:\link\a.jpg", fake_canonicalize);
    assert_eq!(resolved.as_deref(), Some(r"E:\real\a.jpg"));
}

#[test]
fn recycle_through_a_link_to_a_removable_volume_is_refused_and_permanent_delete_follows_the_target() {
    let path = r"C:\link\a.jpg";
    // The string path alone says "fixed C:" (the C1 hazard); the resolved path says removable E:.
    assert!(can_recycle_with(path, drive_of, mount_of));
    let resolved = resolve_final_path_with(path, fake_canonicalize).expect("resolved");
    assert!(!can_recycle_with(&resolved, drive_of, mount_of));
    assert!(is_permanent_delete_allowed_with(&resolved, drive_of, mount_of));
}

#[test]
fn missing_folders_are_walked_up_to_the_deepest_existing_one() {
    // `C:\link\missing...` does not exist; `C:\link` does and resolves to `E:\real`.
    let resolved = resolve_final_path_with(r"C:\link\missing\deeper\a.jpg", |dir| {
        if dir.to_string_lossy() == r"C:\link" {
            Ok(PathBuf::from(r"\\?\E:\real"))
        } else {
            Err(io::Error::from(io::ErrorKind::NotFound))
        }
    });
    assert_eq!(resolved.as_deref(), Some(r"E:\real\missing\deeper\a.jpg"));
}

#[test]
fn only_containing_folders_are_resolved_never_the_leaf() {
    let asked = RefCell::new(Vec::new());
    let resolved = resolve_final_path_with(r"C:\photos\link-leaf", |dir| {
        asked.borrow_mut().push(dir.to_string_lossy().into_owned());
        fake_canonicalize(dir)
    });
    assert_eq!(resolved.as_deref(), Some(r"C:\photos\link-leaf"));
    assert_eq!(asked.into_inner(), vec![r"C:\photos".to_owned()]);
}

#[test]
fn unresolvable_link_chain_fails_closed() {
    assert_eq!(resolve_final_path_with(r"C:\loop\a.jpg", fake_canonicalize), None);
    let denied = |_: &Path| Err(io::Error::from(io::ErrorKind::PermissionDenied));
    assert_eq!(resolve_final_path_with(r"C:\photos\a.jpg", denied), None);
    // Nothing exists up to the root: no positively known volume.
    let nothing = |_: &Path| Err(io::Error::from(io::ErrorKind::NotFound));
    assert_eq!(resolve_final_path_with(r"C:\photos\a.jpg", nothing), None);
}

#[test]
fn spelling_rules_for_drive_unc_and_verbatim_inputs() {
    assert_eq!(
        resolve_final_path_with(r"C:\a.jpg", fake_canonicalize).as_deref(),
        Some(r"C:\a.jpg")
    );
    let unc = |_: &Path| Ok(PathBuf::from(r"\\?\UNC\srv\share\dir"));
    assert_eq!(
        resolve_final_path_with(r"Z:\dir\a.jpg", unc).as_deref(),
        Some(r"\\srv\share\dir\a.jpg")
    );
    assert_eq!(
        resolve_final_path_with(r"\\?\C:\x\a.jpg", |dir| Ok(dir.to_path_buf())).as_deref(),
        Some(r"\\?\C:\x\a.jpg")
    );
    let volume = |_: &Path| Ok(PathBuf::from(r"\\?\Volume{1234}\dir"));
    assert_eq!(
        resolve_final_path_with(r"C:\d\a.jpg", volume).as_deref(),
        Some(r"\\?\Volume{1234}\dir\a.jpg")
    );
    // A volume root has no containing folder.
    assert_eq!(
        resolve_final_path_with(r"C:\", fake_canonicalize).as_deref(),
        Some(r"C:\")
    );
}

#[test]
#[ignore = "native: creates a junction inside a TempRoot with cmd mklink; read-only volume queries only, no Recycle Bin"]
fn query_mount_point_sees_the_volume_through_a_real_junction() {
    let root = pr_test_support::TempRoot::new("c1-junction");
    let target = root.join("target");
    std::fs::create_dir_all(&target).expect("target");
    let link = root.join("link");
    let status = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .output()
        .expect("cmd");
    assert!(status.status.success(), "mklink /J failed: {status:?}");
    let through = query_mount_point(&link.join("f.jpg").to_string_lossy());
    let direct = query_mount_point(&target.join("f.jpg").to_string_lossy());
    assert!(through.is_some());
    assert_eq!(through, direct);
    // Remove only the link (rmdir on a junction never touches the target).
    let _ = std::process::Command::new("cmd")
        .args(["/c", "rmdir"])
        .arg(&link)
        .output();
}
