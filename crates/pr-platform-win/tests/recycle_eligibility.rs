use pr_platform_win::recycle_bin_settings::DriveType;
use pr_platform_win::recycle_eligibility::{
    can_recycle_with_results, is_permanent_delete_allowed_with_results, is_real_unc,
};
use std::cell::Cell;
use std::io;

fn fixed_mount(path: &str) -> io::Result<Option<String>> {
    let root = path.get(..3).filter(|root| {
        let bytes = root.as_bytes();
        bytes.len() == 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\'
    });
    Ok(root.map(str::to_owned))
}

#[test]
// C# test: CanRecycle_FixedDrive_ReturnsTrue
fn can_recycle_fixed_drive_returns_true() {
    let mut queried = None;
    let actual = can_recycle_with_results(
        r"C:\photos\a.jpg",
        |root| {
            queried = Some(root.to_owned());
            Ok(Some(DriveType::Fixed))
        },
        fixed_mount,
    );
    assert!(actual);
    assert_eq!(queried.as_deref(), Some(r"C:\"));
}

#[test]
// C# test: CanRecycle_NonFixedDrive_ReturnsFalse
fn can_recycle_non_fixed_drive_returns_false() {
    for kind in [
        DriveType::Removable,
        DriveType::Network,
        DriveType::CdRom,
        DriveType::Ram,
        DriveType::NoRootDirectory,
        DriveType::Unknown,
    ] {
        assert!(!can_recycle_with_results(
            r"E:\DCIM\a.jpg",
            |_| Ok(Some(kind)),
            fixed_mount,
        ));
    }
}

#[test]
// C# test: CanRecycle_UnknownDrive_ReturnsFalse
fn can_recycle_unknown_drive_returns_false() {
    assert!(!can_recycle_with_results(r"E:\DCIM\a.jpg", |_| Ok(None), fixed_mount,));
}

#[test]
// C# test: CanRecycle_UncPath_ReturnsFalse
fn can_recycle_unc_path_returns_false_without_querying_drive() {
    for path in [r"\\nas\share\a.jpg", r"\\?\UNC\nas\share\a.jpg"] {
        let mut queried = false;
        assert!(!can_recycle_with_results(
            path,
            |_| {
                queried = true;
                Ok(Some(DriveType::Fixed))
            },
            fixed_mount,
        ));
        assert!(!queried);
    }
}

#[test]
// C# test: CanRecycle_ExtendedLocalPath_QueriesDriveRoot
fn can_recycle_extended_drive_path_queries_normalized_full_path() {
    let mut mount_received = None;
    let mut type_received = None;
    assert!(can_recycle_with_results(
        r"\\?\D:\photos\a.jpg",
        |root| {
            type_received = Some(root.to_owned());
            Ok(Some(DriveType::Fixed))
        },
        |path| {
            mount_received = Some(path.to_owned());
            Ok(Some(r"D:\".to_owned()))
        },
    ));
    assert_eq!(mount_received.as_deref(), Some(r"D:\photos\a.jpg"));
    assert_eq!(type_received.as_deref(), Some(r"D:\"));
}

#[test]
// C# test: CanRecycle_JunctionToNonFixedVolume_ReturnsFalse
fn can_recycle_junction_to_non_fixed_volume_uses_resolved_mount() {
    let mut queried = None;
    let actual = can_recycle_with_results(
        r"C:\link\usb\x.jpg",
        |root| {
            queried = Some(root.to_owned());
            Ok(Some(DriveType::Removable))
        },
        |_| Ok(Some(r"E:\".to_owned())),
    );
    assert!(!actual);
    assert_eq!(queried.as_deref(), Some(r"E:\"));
}

#[test]
// C# test: CanRecycle_MountedFolder_UsesMountPointType
fn can_recycle_mounted_folder_uses_its_own_drive_type() {
    for (mount, kind, expected) in [
        (r"C:\mnt\usb\", DriveType::Removable, false),
        (r"C:\mnt\fixed\", DriveType::Fixed, true),
    ] {
        let mut received = None;
        assert_eq!(
            can_recycle_with_results(
                r"C:\mnt\target\x.jpg",
                |root| {
                    received = Some(root.to_owned());
                    Ok(Some(kind))
                },
                |_| Ok(Some(mount.to_owned())),
            ),
            expected
        );
        assert_eq!(received.as_deref(), Some(mount));
    }
}

#[test]
// C# test: CanRecycle_JunctionToFixedVolume_ReturnsTrue
fn can_recycle_junction_to_fixed_volume_returns_true() {
    assert!(can_recycle_with_results(
        r"C:\link\x.jpg",
        |_| Ok(Some(DriveType::Fixed)),
        |_| Ok(Some(r"D:\".to_owned())),
    ));
}

#[test]
// C# test: CanRecycle_UnresolvableOrUncMountPoint_ReturnsFalse
fn can_recycle_unresolvable_or_unc_mount_is_refused_without_drive_query() {
    for mount in [None, Some(String::new()), Some(r"\\nas\share\".to_owned())] {
        let mut queried = false;
        assert!(!can_recycle_with_results(
            r"C:\link\x.jpg",
            |_| {
                queried = true;
                Ok(Some(DriveType::Fixed))
            },
            |_| Ok(mount.clone()),
        ));
        assert!(!queried);
    }
}

#[test]
// C# test: CanRecycle_ResolverThrows_ReturnsFalse
fn can_recycle_mount_or_drive_query_failure_returns_false() {
    assert!(!can_recycle_with_results(
        r"C:\a.jpg",
        |_| Ok(Some(DriveType::Fixed)),
        |_| Err(io::Error::other("mount lookup failed")),
    ));
    assert!(!can_recycle_with_results(
        r"C:\a.jpg",
        |_| Err(io::Error::other("drive lookup failed")),
        fixed_mount,
    ));
}

#[test]
// C# test: CanRecycle_RelativePath_ResolverReceivesFullPath
fn can_recycle_relative_path_is_made_absolute_before_mount_lookup() {
    let mut received = None;
    assert!(can_recycle_with_results(
        r"relative\.\folder\..\a.jpg",
        |_| Ok(Some(DriveType::Fixed)),
        |full| {
            received = Some(full.to_owned());
            Ok(Some(r"C:\".to_owned()))
        },
    ));
    assert!(
        received
            .as_deref()
            .is_some_and(|path| path.contains(r"relative\a.jpg") && !path.contains(r"\folder\..\"))
    );
}

#[test]
// C# test: Eligibility_PathThatCannotBeResolved_IsNeverRecyclableNorPermanentlyDeletable
fn eligibility_path_with_nul_is_never_allowed() {
    let path = "C:\\bad\0path";
    let query = |_: &str| Ok(Some(DriveType::Fixed));
    let mount = |_: &str| Ok(Some(r"C:\".to_owned()));
    assert!(!can_recycle_with_results(path, query, mount));
    assert!(!is_permanent_delete_allowed_with_results(path, query, mount));
}

#[test]
// C# test: IsPermanentDeleteAllowed_WithoutAResolver_JudgesTheTextualDriveRoot
fn permanent_delete_requires_positive_non_fixed_type() {
    for kind in [
        DriveType::Removable,
        DriveType::Network,
        DriveType::CdRom,
        DriveType::Ram,
    ] {
        assert!(is_permanent_delete_allowed_with_results(
            r"E:\DCIM\a.jpg",
            |_| Ok(Some(kind)),
            fixed_mount,
        ));
    }
    for kind in [DriveType::Fixed, DriveType::Unknown, DriveType::NoRootDirectory] {
        assert!(!is_permanent_delete_allowed_with_results(
            r"E:\DCIM\a.jpg",
            |_| Ok(Some(kind)),
            fixed_mount,
        ));
    }
    assert!(!is_permanent_delete_allowed_with_results(
        r"E:\DCIM\a.jpg",
        |_| Ok(None),
        fixed_mount,
    ));
}

#[test]
// C# test: IsPermanentDeleteAllowed_MountPointLookupThrowsIo_RefusesFailClosed
fn permanent_delete_mount_or_drive_query_failure_refuses() {
    assert!(!is_permanent_delete_allowed_with_results(
        r"E:\a.jpg",
        |_| Ok(Some(DriveType::Removable)),
        |_| Err(io::Error::other("mount lookup failed")),
    ));
    assert!(!is_permanent_delete_allowed_with_results(
        r"E:\a.jpg",
        |_| Err(io::Error::other("drive lookup failed")),
        fixed_mount,
    ));
}

#[test]
// C# test: IsPermanentDeleteAllowed_RealUnc_ReturnsTrue
fn permanent_delete_real_unc_is_allowed_without_queries() {
    for path in [r"\\nas\share\a.jpg", r"\\?\UNC\nas\share\a.jpg"] {
        let queried = Cell::new(false);
        assert!(is_permanent_delete_allowed_with_results(
            path,
            |_| {
                queried.set(true);
                Ok(None)
            },
            |_| {
                queried.set(true);
                Ok(None)
            },
        ));
        assert!(!queried.get());
    }
}

#[test]
// C# test: IsRealUnc_R44_ExcludesVolumeGuidAndDevicePaths
fn is_real_unc_recognizes_only_unc_namespaces() {
    assert!(is_real_unc(r"\\server\share\a.jpg"));
    assert!(is_real_unc(r"\\?\UNC\server\share\a.jpg"));
    assert!(is_real_unc(r"\\?\unc\server\share\a.jpg"));
    assert!(!is_real_unc(r"\\?\Volume{00000000-0000-0000-0000-000000000000}\"));
    assert!(!is_real_unc(r"\\.\PhysicalDrive0"));
    assert!(!is_real_unc(r"\\?\C:\photos\a.jpg"));
}

#[test]
// C# test: CanRecycle_UncPathWithResolver_ReturnsFalse
fn permanent_delete_unc_mount_is_allowed_but_recycle_unc_mount_is_refused() {
    let queried = Cell::new(false);
    assert!(!can_recycle_with_results(
        r"C:\junction\a.jpg",
        |_: &str| {
            queried.set(true);
            Ok(Some(DriveType::Fixed))
        },
        |_: &str| Ok(Some(r"\\nas\share\".to_owned())),
    ));
    assert!(is_permanent_delete_allowed_with_results(
        r"C:\junction\a.jpg",
        |_: &str| {
            queried.set(true);
            Ok(None)
        },
        |_: &str| Ok(Some(r"\\nas\share\".to_owned())),
    ));
    assert!(!queried.get());
}
