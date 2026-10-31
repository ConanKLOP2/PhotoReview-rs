//! Port of PhotoReview.Platform.Windows/RecycleBinCapacity.cs: injected adapters only.

use pr_platform_win::recycle_bin_settings::{
    DriveType, RecycleBinSettingsSource, WindowsRecycleBinSettingsSource, drive_type_from_win32,
    resolve_final_path_with,
};

// C# GetDriveType mapping; recycle_bin_settings.rs:356 body and :357-362 arm deletions.
#[test]
fn drive_type_mapping_keeps_all_codes_and_unknown_values_distinct() {
    for (code, expected) in [
        (0, DriveType::Unknown),
        (1, DriveType::NoRootDirectory),
        (2, DriveType::Removable),
        (3, DriveType::Fixed),
        (4, DriveType::Network),
        (5, DriveType::CdRom),
        (6, DriveType::Ram),
        (7, DriveType::Unknown),
        (u32::MAX, DriveType::Unknown),
    ] {
        assert_eq!(drive_type_from_win32(code), expected, "{code}");
    }
}

// FIX-RECYCLE-C1 normalization; recycle_bin_settings.rs:258/:269/:286 body and match guards.
#[test]
#[cfg(windows)]
fn normalization_rebuilds_missing_parents_and_preserves_verbatim_or_unc_spelling() {
    use std::io;
    use std::path::{Path, PathBuf};
    for (input, canonical, expected) in [
        (r"Q:\source\photo", r"\\?\R:\target", r"R:\target\photo"),
        (r"Q:\source\photo", r"\\?\UNC\server\share", r"\\server\share\photo"),
        (r"\\?\Q:\source\photo", r"\\?\R:\target", r"\\?\R:\target\photo"),
        (r"Q:\source\photo", r"\\?\Volume{fake}", r"\\?\Volume{fake}\photo"),
    ] {
        assert_eq!(
            resolve_final_path_with(input, |_| Ok(PathBuf::from(canonical))).as_deref(),
            Some(expected)
        );
    }
    let mut asked = Vec::new();
    let answer = resolve_final_path_with(r"Q:\source\missing\child\photo", |folder| {
        asked.push(folder.to_path_buf());
        if folder == Path::new(r"Q:\source") {
            Ok(PathBuf::from(r"R:\target"))
        } else {
            Err(io::ErrorKind::NotFound.into())
        }
    });
    assert_eq!(answer.as_deref(), Some(r"R:\target\missing\child\photo"));
    assert_eq!(
        asked,
        [
            PathBuf::from(r"Q:\source\missing\child"),
            PathBuf::from(r"Q:\source\missing"),
            PathBuf::from(r"Q:\source")
        ]
    );
    let mut calls = 0;
    assert_eq!(
        resolve_final_path_with(r"Q:\source\photo", |_| {
            calls += 1;
            Err(io::ErrorKind::PermissionDenied.into())
        }),
        None
    );
    assert_eq!(calls, 1);
}

fn source(total: Option<u64>) -> WindowsRecycleBinSettingsSource {
    WindowsRecycleBinSettingsSource::with_adapters(
        |_, _, _| panic!("registry must not be queried"),
        |path| {
            assert_eq!(path, "input");
            Some("mount".into())
        },
        |mount| {
            assert_eq!(mount, "mount");
            Some(r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\\".into())
        },
        |mount| {
            assert_eq!(mount, "mount");
            Some(DriveType::Removable)
        },
        move |mount| {
            assert_eq!(mount, "mount");
            total
        },
    )
}

// recycle_bin_settings.rs:182/:188/:207 body replacements and :209 quota ==/>/|| operators.
// C# volume adapters propagate unknown results and reject totals outside positive Int64.
#[test]
fn volume_adapters_forward_mount_and_validate_quota_boundaries() {
    let settings = source(Some(42));
    assert_eq!(
        settings.get_volume_guid("input").as_deref(),
        Some("{01234567-89ab-cdef-0123-456789abcdef}")
    );
    assert_eq!(settings.get_volume_drive_type("input"), Some(DriveType::Removable));
    for (total, expected) in [
        (None, None),
        (Some(0), None),
        (Some(1), Some(1)),
        (Some(42), Some(42)),
        (Some(i64::MAX as u64), Some(i64::MAX)),
        (Some(i64::MAX as u64 + 1), None),
        (Some(u64::MAX), None),
    ] {
        assert_eq!(source(total).get_user_quota_bytes("input"), expected, "{total:?}");
    }
    let absent = WindowsRecycleBinSettingsSource::with_adapters(
        |_, _, _| panic!("registry"),
        |_| None,
        |_| panic!("volume"),
        |_| panic!("type"),
        |_| panic!("quota"),
    );
    assert_eq!(absent.get_volume_guid("input"), None);
    assert_eq!(absent.get_volume_drive_type("input"), None);
    assert_eq!(absent.get_user_quota_bytes("input"), None);
}

// recycle_bin_settings.rs:131 extract_guid body and :132-147 byte/length guard operators; C# validates brace GUIDs.
#[test]
fn extract_guid_accepts_case_insensitive_prefix_and_rejects_each_malformed_component() {
    let guid = "{01234567-89aB-cdef-0123-456789abcdef}";
    for prefix in [r"\\?\Volume", r"\\?\vOlUmE"] {
        assert_eq!(
            WindowsRecycleBinSettingsSource::extract_guid(&format!("{prefix}{guid}\\")),
            Some(guid.to_owned())
        );
    }
    for invalid in [
        "",
        "Volume{01234567-89ab-cdef-0123-456789abcdef}",
        r"\\?\Volume{}",
        r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef",
        r"\\?\Volume{01234567-89ab-cdef-0123-456789abcde}",
    ] {
        assert_eq!(
            WindowsRecycleBinSettingsSource::extract_guid(invalid),
            None,
            "{invalid}"
        );
    }
    for index in [1, 8, 9, 14, 19, 24, 36] {
        let mut bad = guid.as_bytes().to_vec();
        bad[index] = if [9, 14, 19, 24].contains(&index) { b'0' } else { b'g' };
        assert_eq!(
            WindowsRecycleBinSettingsSource::extract_guid(&format!(r"\\?\Volume{}\", String::from_utf8(bad).unwrap())),
            None
        );
    }
}
