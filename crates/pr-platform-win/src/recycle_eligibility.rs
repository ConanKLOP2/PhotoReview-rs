//! Port of `PhotoReview.Platform.Windows/WindowsRecycleBin.cs` (`RecycleEligibility`).
//!
//! The policy is intentionally separate from Shell operations. Callers supply the mount-point and drive-type
//! queries so tests can exercise every decision without touching the real Recycle Bin.

use crate::recycle_bin_settings::{DriveType, query_drive_type, query_mount_point};
use std::io;
use std::path::{Component, Path, PathBuf};

/// Returns whether the path is on a positively identified fixed volume and can be recycled safely.
pub fn can_recycle(path: &str) -> bool {
    can_recycle_with(path, query_drive_type, query_mount_point)
}

/// Recycle eligibility with injected read-only volume queries.
pub fn can_recycle_with(
    path: &str,
    mut drive_type_of: impl FnMut(&str) -> Option<DriveType>,
    mut mount_point_of: impl FnMut(&str) -> Option<String>,
) -> bool {
    let Some(full) = normalized_full_path(path) else {
        return false;
    };
    if is_real_unc(&full) {
        return false;
    }
    let Some(mount) = mount_point_of(&full) else {
        return false;
    };
    if mount.is_empty() || is_real_unc(&mount) {
        return false;
    }
    drive_type_of(&mount) == Some(DriveType::Fixed)
}

/// Returns whether a permanent delete is allowed because the target is positively known to have no Recycle Bin.
pub fn is_permanent_delete_allowed(path: &str) -> bool {
    is_permanent_delete_allowed_with(path, query_drive_type, query_mount_point)
}

/// Permanent-delete policy with injected read-only volume queries.
pub fn is_permanent_delete_allowed_with(
    path: &str,
    mut drive_type_of: impl FnMut(&str) -> Option<DriveType>,
    mut mount_point_of: impl FnMut(&str) -> Option<String>,
) -> bool {
    let Some(full) = normalized_full_path(path) else {
        return false;
    };
    if is_real_unc(&full) {
        return true;
    }
    let Some(mount) = mount_point_of(&full) else {
        return false;
    };
    if mount.is_empty() {
        return false;
    }
    if is_real_unc(&mount) {
        return true;
    }
    matches!(
        drive_type_of(&mount),
        Some(DriveType::Removable | DriveType::Network | DriveType::CdRom | DriveType::Ram)
    )
}

/// R44: recognize actual UNC syntax while treating volume-GUID and device namespaces as local forms.
pub fn is_real_unc(path: &str) -> bool {
    const EXTENDED_UNC: &str = "\\\\?\\UNC\\";
    const EXTENDED: &str = "\\\\?\\";
    const DEVICE: &str = "\\\\.\\";
    path.get(..EXTENDED_UNC.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(EXTENDED_UNC))
        || (path.starts_with("\\\\") && !path.starts_with(EXTENDED) && !path.starts_with(DEVICE))
}

fn normalized_full_path(path: &str) -> Option<String> {
    if path.trim().is_empty() || path.contains('\0') {
        return None;
    }
    let absolute = std::path::absolute(Path::new(path)).ok()?;
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if normalized.file_name().is_some() {
                    let _ = normalized.pop();
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    let mut full = normalized.to_string_lossy().into_owned();
    // Match C#'s Path.GetFullPath handling for extended drive-letter paths while preserving other namespaces.
    if full.starts_with("\\\\?\\") && full.len() >= 6 && full.as_bytes().get(5) == Some(&b':') {
        full = full[4..].to_owned();
    }
    Some(full)
}

/// Adapter-shaped wrappers are useful for callers that need to preserve an I/O failure as a closed decision.
pub fn can_recycle_with_results(
    path: &str,
    mut drive_type_of: impl FnMut(&str) -> io::Result<Option<DriveType>>,
    mut mount_point_of: impl FnMut(&str) -> io::Result<Option<String>>,
) -> bool {
    can_recycle_with(
        path,
        |root| drive_type_of(root).ok().flatten(),
        |full| mount_point_of(full).ok().flatten(),
    )
}

/// Result-returning counterpart for permanent-delete policy; lookup errors always fail closed.
pub fn is_permanent_delete_allowed_with_results(
    path: &str,
    mut drive_type_of: impl FnMut(&str) -> io::Result<Option<DriveType>>,
    mut mount_point_of: impl FnMut(&str) -> io::Result<Option<String>>,
) -> bool {
    is_permanent_delete_allowed_with(
        path,
        |root| drive_type_of(root).ok().flatten(),
        |full| mount_point_of(full).ok().flatten(),
    )
}
