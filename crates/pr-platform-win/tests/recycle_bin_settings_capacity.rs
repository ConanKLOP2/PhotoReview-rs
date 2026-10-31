#[path = "../src/recycle_bin_capacity.rs"]
mod recycle_bin_capacity;
#[path = "../src/recycle_bin_settings.rs"]
mod recycle_bin_settings;

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};

use pr_core::abstractions::{Log, NullLog};
use recycle_bin_capacity::{RecycleBinCapacityGuard, RecycleCapacityVerdict as Verdict};
use recycle_bin_settings::{
    DriveType, RecycleBinPolicy, RecycleBinSettingsSource, RecycleBinVolumeSettings, RegistryHive, RegistryValue,
    WindowsRecycleBinSettingsSource, read_dword_with_query,
};

const PHOTO: &str = r"C:\photos\a.jpg";
const VOLUME_GUID: &str = "{0a089459-c105-4107-99e6-657d2fddd42b}";
const MIB: i64 = RecycleBinCapacityGuard::MEBIBYTE;
const GIB: i64 = 1024 * MIB;
const POLICY_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer";
const BIT_BUCKET_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\BitBucket";

type RegistryMap = HashMap<(RegistryHive, String, String), RegistryValue>;

fn settings_source(values: RegistryMap) -> WindowsRecycleBinSettingsSource {
    WindowsRecycleBinSettingsSource::with_adapters(
        move |hive, key, name| Ok(values.get(&(hive, key.to_owned(), name.to_owned())).copied()),
        |_| Some(r"C:\".to_owned()),
        |_| Some(format!(r"\\?\Volume{}\", VOLUME_GUID)),
        |_| Some(DriveType::Fixed),
        |_| Some((100 * GIB) as u64),
    )
}

#[derive(Clone)]
struct FakeSettings {
    policy: RecycleBinPolicy,
    guid: Option<String>,
    drive_type: Option<DriveType>,
    volume: RecycleBinVolumeSettings,
    quota_bytes: Option<i64>,
    read_error: Option<io::ErrorKind>,
    quota_calls: Arc<Mutex<usize>>,
}

impl Default for FakeSettings {
    fn default() -> Self {
        Self {
            policy: RecycleBinPolicy::default(),
            guid: Some(VOLUME_GUID.to_owned()),
            drive_type: Some(DriveType::Fixed),
            volume: RecycleBinVolumeSettings {
                nuke_on_delete: Some(0),
                max_capacity_mb: Some(10_240),
            },
            quota_bytes: Some(100 * GIB),
            read_error: None,
            quota_calls: Arc::new(Mutex::new(0)),
        }
    }
}

impl RecycleBinSettingsSource for FakeSettings {
    fn read_policy(&self) -> io::Result<RecycleBinPolicy> {
        if let Some(kind) = self.read_error {
            return Err(io::Error::new(kind, "fake registry read failure"));
        }
        Ok(self.policy)
    }

    fn get_volume_guid(&self, _path: &str) -> Option<String> {
        self.guid.clone()
    }

    fn get_volume_drive_type(&self, _path: &str) -> Option<DriveType> {
        self.drive_type
    }

    fn read_volume(&self, _volume_guid: &str) -> io::Result<RecycleBinVolumeSettings> {
        Ok(self.volume)
    }

    fn get_user_quota_bytes(&self, _path: &str) -> Option<i64> {
        *self.quota_calls.lock().expect("quota counter lock") += 1;
        self.quota_bytes
    }
}

fn evaluate(settings: &FakeSettings, size: Option<i64>) -> Verdict {
    RecycleBinCapacityGuard::evaluate(PHOTO, size, settings, Some(&NullLog)).expect("valid test path")
}

#[test]
fn read_policy_nothing_configured_is_all_off() {
    // C# twin: RecycleBinPlatformGapTests.ReadPolicy_NothingConfigured_IsAllOff.
    assert_eq!(
        settings_source(RegistryMap::new()).read_policy().unwrap(),
        RecycleBinPolicy::default()
    );
}

#[test]
fn read_policy_no_recycle_files_in_either_hive_disables_the_bin() {
    // C# twin: RecycleBinPlatformGapTests.ReadPolicy_NoRecycleFilesInEitherHive_DisablesTheBin.
    for hive in [RegistryHive::CurrentUser, RegistryHive::LocalMachine] {
        let mut values = RegistryMap::new();
        values.insert(
            (hive, POLICY_KEY.to_owned(), "NoRecycleFiles".to_owned()),
            RegistryValue::Dword(1),
        );
        assert!(settings_source(values).read_policy().unwrap().no_recycle_files);
    }
}

#[test]
fn read_policy_no_recycle_files_zero_leaves_the_bin_on() {
    // C# twin: RecycleBinPlatformGapTests.ReadPolicy_NoRecycleFilesZero_LeavesTheBinOn.
    let mut values = RegistryMap::new();
    for hive in [RegistryHive::CurrentUser, RegistryHive::LocalMachine] {
        values.insert(
            (hive, POLICY_KEY.to_owned(), "NoRecycleFiles".to_owned()),
            RegistryValue::Dword(0),
        );
    }
    assert!(!settings_source(values).read_policy().unwrap().no_recycle_files);
}

#[test]
fn read_policy_recycle_bin_size_uses_the_stricter_of_user_and_machine() {
    // C# twin: RecycleBinPlatformGapTests.ReadPolicy_RecycleBinSize_UsesTheStricterOfUserAndMachine.
    for (user, machine, expected) in [
        (Some(10), None, Some(10)),
        (None, Some(7), Some(7)),
        (Some(10), Some(7), Some(7)),
        (Some(5), Some(20), Some(5)),
    ] {
        let mut values = RegistryMap::new();
        for (hive, value) in [(RegistryHive::CurrentUser, user), (RegistryHive::LocalMachine, machine)] {
            if let Some(value) = value {
                values.insert(
                    (hive, POLICY_KEY.to_owned(), "RecycleBinSize".to_owned()),
                    RegistryValue::Dword(value),
                );
            }
        }
        assert_eq!(
            settings_source(values).read_policy().unwrap().max_size_percent,
            expected
        );
    }
}

#[test]
fn read_policy_legacy_nuke_on_delete_reads_the_bit_bucket_key_of_the_user() {
    // C# twin: RecycleBinPlatformGapTests.ReadPolicy_LegacyGlobalNukeOnDelete_ReadsTheBitBucketKeyOfTheUser.
    let mut values = RegistryMap::new();
    values.insert(
        (
            RegistryHive::CurrentUser,
            BIT_BUCKET_KEY.to_owned(),
            "NukeOnDelete".to_owned(),
        ),
        RegistryValue::Dword(1),
    );
    assert!(
        settings_source(values)
            .read_policy()
            .unwrap()
            .legacy_global_nuke_on_delete
    );
    assert!(
        !settings_source(RegistryMap::new())
            .read_policy()
            .unwrap()
            .legacy_global_nuke_on_delete
    );
}

#[test]
fn read_policy_dword_with_high_bit_set_is_read_as_unsigned() {
    // C# twin: RecycleBinPlatformGapTests.ReadPolicy_DwordWithHighBitSet_IsReadAsUnsigned.
    let mut values = RegistryMap::new();
    values.insert(
        (
            RegistryHive::CurrentUser,
            POLICY_KEY.to_owned(),
            "RecycleBinSize".to_owned(),
        ),
        RegistryValue::Dword(u32::MAX),
    );
    assert_eq!(
        settings_source(values).read_policy().unwrap().max_size_percent,
        Some(i64::from(u32::MAX))
    );
}

#[test]
fn read_policy_non_dword_value_errors_and_capacity_guard_fails_closed() {
    // C# twin: RecycleBinPlatformGapTests.ReadPolicy_ValueThatIsNotADword_ThrowsAndTheGuardFailsClosed.
    let mut values = RegistryMap::new();
    values.insert(
        (
            RegistryHive::LocalMachine,
            POLICY_KEY.to_owned(),
            "NoRecycleFiles".to_owned(),
        ),
        RegistryValue::Other,
    );
    let source = settings_source(values);
    assert_eq!(source.read_policy().unwrap_err().kind(), io::ErrorKind::InvalidData);
    let fake = FakeSettings {
        read_error: Some(io::ErrorKind::InvalidData),
        ..FakeSettings::default()
    };
    assert_eq!(evaluate(&fake, Some(1)), Verdict::Unknown);
}

#[test]
fn read_volume_reads_both_values_of_that_volumes_key() {
    // C# twin: RecycleBinPlatformGapTests.ReadVolume_ReadsBothValuesOfThatVolumesKey.
    let volume_key = format!("{BIT_BUCKET_KEY}\\Volume\\{VOLUME_GUID}");
    let mut values = RegistryMap::new();
    values.insert(
        (RegistryHive::CurrentUser, volume_key.clone(), "NukeOnDelete".to_owned()),
        RegistryValue::Dword(1),
    );
    values.insert(
        (RegistryHive::CurrentUser, volume_key, "MaxCapacity".to_owned()),
        RegistryValue::Dword(5120),
    );
    assert_eq!(
        settings_source(values).read_volume(VOLUME_GUID).unwrap(),
        RecycleBinVolumeSettings {
            nuke_on_delete: Some(1),
            max_capacity_mb: Some(5120),
        }
    );
    assert_eq!(
        settings_source(RegistryMap::new()).read_volume(VOLUME_GUID).unwrap(),
        RecycleBinVolumeSettings::default()
    );
}

#[test]
fn read_volume_blank_volume_name_errors() {
    // C# twin: RecycleBinPlatformGapTests.ReadVolume_BlankVolumeName_Throws.
    for value in ["", "  "] {
        assert_eq!(
            settings_source(RegistryMap::new())
                .read_volume(value)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}

#[test]
fn injected_volume_adapters_supply_mount_point_guid_drive_type_and_quota() {
    // C# twin: RecycleBinPlatformGapTests.VolumeQueries_ForAFileInTheTempFolder_AgreeWithEachOther (fakes only).
    let seen_mounts = Arc::new(Mutex::new(Vec::new()));
    let mount_seen = seen_mounts.clone();
    let drive_seen = seen_mounts.clone();
    let name_seen = seen_mounts.clone();
    let quota_seen = seen_mounts.clone();
    let source = WindowsRecycleBinSettingsSource::with_adapters(
        |_, _, _| Ok(None),
        move |path| {
            mount_seen.lock().unwrap().push(path.to_owned());
            Some(r"C:\mounted\".to_owned())
        },
        move |mount| {
            name_seen.lock().unwrap().push(mount.to_owned());
            Some(format!(r"\\?\Volume{}\", VOLUME_GUID))
        },
        move |mount| {
            drive_seen.lock().unwrap().push(mount.to_owned());
            Some(DriveType::Fixed)
        },
        move |mount| {
            quota_seen.lock().unwrap().push(mount.to_owned());
            Some((100 * GIB) as u64)
        },
    );
    assert_eq!(source.get_volume_guid(PHOTO).as_deref(), Some(VOLUME_GUID));
    assert_eq!(source.get_volume_drive_type(PHOTO), Some(DriveType::Fixed));
    assert_eq!(source.get_user_quota_bytes(PHOTO), Some(100 * GIB));
    assert_eq!(
        seen_mounts.lock().unwrap().as_slice(),
        &[PHOTO, r"C:\mounted\", PHOTO, r"C:\mounted\", PHOTO, r"C:\mounted\"]
    );
}

#[test]
fn extract_guid_parses_volume_name_and_rejects_malformed_values() {
    // C# twin: RecycleBinCapacityGuardTests.ExtractGuid_ParsesVolumeName.
    for (name, expected) in [
        (r"\\?\Volume{0a089459-c105-4107-99e6-657d2fddd42b}\", Some(VOLUME_GUID)),
        (r"\\?\Volume{not-a-guid}\", None),
        (r"C:\", None),
        (r"\\?\Volume{0a089459-c105-4107-99e6-657d2fddd42b", None),
    ] {
        assert_eq!(WindowsRecycleBinSettingsSource::extract_guid(name).as_deref(), expected);
    }
}

#[test]
fn evaluate_small_file_bin_on_fits_and_uses_volume_settings() {
    // C# twin: RecycleBinCapacityGuardTests.Evaluate_SmallFileBinOn_Fits.
    let settings = FakeSettings::default();
    assert_eq!(evaluate(&settings, Some(20 * MIB)), Verdict::Fits);
    assert_eq!(*settings.quota_calls.lock().unwrap(), 0);
}

#[test]
fn evaluate_non_fixed_or_unresolvable_volume_is_unknown_for_known_and_unknown_size() {
    // C# twin: RecycleBinCapacityGuardTests.Evaluate_NonFixedVolume_Unknown.
    for kind in [
        None,
        Some(DriveType::Removable),
        Some(DriveType::Network),
        Some(DriveType::CdRom),
        Some(DriveType::Unknown),
    ] {
        let settings = FakeSettings {
            drive_type: kind,
            ..FakeSettings::default()
        };
        assert_eq!(evaluate(&settings, None), Verdict::Unknown);
        assert_eq!(evaluate(&settings, Some(1)), Verdict::Unknown);
    }
}

#[test]
fn evaluate_file_larger_than_max_capacity_is_too_large() {
    // C# twin: RecycleBinCapacityGuardTests.Evaluate_FileLargerThanMaxCapacity_TooLarge.
    let settings = FakeSettings {
        volume: RecycleBinVolumeSettings {
            nuke_on_delete: Some(0),
            max_capacity_mb: Some(100),
        },
        ..FakeSettings::default()
    };
    assert_eq!(evaluate(&settings, Some(150 * MIB)), Verdict::TooLarge);
}

#[test]
fn evaluate_safety_margin_boundary_matches_csharp() {
    // C# twin: RecycleBinCapacityGuardTests.Evaluate_AtSafetyMargin_Boundary.
    let settings = FakeSettings {
        volume: RecycleBinVolumeSettings {
            nuke_on_delete: Some(0),
            max_capacity_mb: Some(100),
        },
        ..FakeSettings::default()
    };
    assert_eq!(evaluate(&settings, Some(100 * MIB - MIB)), Verdict::Fits);
    assert_eq!(evaluate(&settings, Some(100 * MIB - MIB + 1)), Verdict::TooLarge);
}

#[test]
fn evaluate_volume_nuke_on_delete_disables_bin_with_or_without_size() {
    // C# twin: RecycleBinCapacityGuardTests.Evaluate_NukeOnDelete_BinDisabled.
    let settings = FakeSettings {
        volume: RecycleBinVolumeSettings {
            nuke_on_delete: Some(1),
            max_capacity_mb: Some(10_240),
        },
        ..FakeSettings::default()
    };
    assert_eq!(evaluate(&settings, Some(1)), Verdict::BinDisabled);
    assert_eq!(evaluate(&settings, None), Verdict::BinDisabled);
}

#[test]
fn evaluate_no_recycle_files_policy_disables_bin() {
    // C# twin: RecycleBinCapacityGuardTests.Evaluate_NoRecycleFilesPolicy_BinDisabled.
    let settings = FakeSettings {
        policy: RecycleBinPolicy {
            no_recycle_files: true,
            ..RecycleBinPolicy::default()
        },
        ..FakeSettings::default()
    };
    assert_eq!(evaluate(&settings, None), Verdict::BinDisabled);
}

#[test]
fn evaluate_legacy_global_nuke_disables_bin() {
    // C# twin: RecycleBinCapacityGuardTests.Evaluate_LegacyGlobalNuke_BinDisabled.
    let settings = FakeSettings {
        policy: RecycleBinPolicy {
            legacy_global_nuke_on_delete: true,
            ..RecycleBinPolicy::default()
        },
        ..FakeSettings::default()
    };
    assert_eq!(evaluate(&settings, Some(1)), Verdict::BinDisabled);
}

#[test]
fn evaluate_no_volume_guid_is_unknown() {
    // C# twin: RecycleBinCapacityGuardTests.Evaluate_NoVolumeGuid_Unknown.
    for guid in [None, Some(String::new())] {
        let settings = FakeSettings {
            guid,
            ..FakeSettings::default()
        };
        assert_eq!(evaluate(&settings, Some(1)), Verdict::Unknown);
    }
}

#[test]
fn evaluate_settings_source_failure_is_unknown_and_logged() {
    // C# twin: RecycleBinCapacityGuardTests.Evaluate_SourceThrows_Unknown.
    #[derive(Default)]
    struct CapturingLog(Mutex<Vec<String>>);
    impl Log for CapturingLog {
        fn enabled(&self) -> bool {
            true
        }
        fn info(&self, _: &str) {}
        fn warn(&self, _: &str) {}
        fn error(&self, message: &str, _: Option<&(dyn std::error::Error + 'static)>) {
            self.0.lock().unwrap().push(message.to_owned());
        }
    }
    let settings = FakeSettings {
        read_error: Some(io::ErrorKind::InvalidData),
        ..FakeSettings::default()
    };
    let log = CapturingLog::default();
    assert_eq!(
        RecycleBinCapacityGuard::evaluate(PHOTO, Some(1), &settings, Some(&log)).unwrap(),
        Verdict::Unknown
    );
    assert_eq!(
        log.0.lock().unwrap().as_slice(),
        &["Recycle Bin settings could not be read: a.jpg"]
    );
}

#[test]
fn evaluate_zero_max_capacity_is_unknown() {
    // C# twin: RecycleBinCapacityGuardTests.Evaluate_ZeroMaxCapacity_Unknown.
    let settings = FakeSettings {
        volume: RecycleBinVolumeSettings {
            nuke_on_delete: Some(0),
            max_capacity_mb: Some(0),
        },
        ..FakeSettings::default()
    };
    assert_eq!(evaluate(&settings, Some(1)), Verdict::Unknown);
}

#[test]
fn evaluate_absent_volume_entry_uses_five_percent_of_user_quota() {
    // C# twin: RecycleBinCapacityGuardTests.Evaluate_NoVolumeEntry_UsesFivePercentOfQuota.
    let settings = FakeSettings {
        volume: RecycleBinVolumeSettings::default(),
        quota_bytes: Some(100 * GIB),
        ..FakeSettings::default()
    };
    assert_eq!(evaluate(&settings, Some(4 * GIB)), Verdict::Fits);
    assert_eq!(evaluate(&settings, Some(6 * GIB)), Verdict::TooLarge);
    assert_eq!(*settings.quota_calls.lock().unwrap(), 2);
}

#[test]
fn evaluate_absent_volume_entry_and_missing_quota_is_unknown() {
    // C# twin: RecycleBinCapacityGuardTests.Evaluate_NoVolumeEntryNoQuota_Unknown.
    let settings = FakeSettings {
        volume: RecycleBinVolumeSettings::default(),
        quota_bytes: None,
        ..FakeSettings::default()
    };
    assert_eq!(evaluate(&settings, Some(1)), Verdict::Unknown);
}

#[test]
fn evaluate_policy_percent_uses_the_stricter_volume_capacity_cap() {
    // C# twin: RecycleBinCapacityGuardTests.Evaluate_PolicyPercent_CapsCapacity.
    let settings = FakeSettings {
        policy: RecycleBinPolicy {
            max_size_percent: Some(1),
            ..RecycleBinPolicy::default()
        },
        volume: RecycleBinVolumeSettings {
            nuke_on_delete: Some(0),
            max_capacity_mb: Some(50_000),
        },
        quota_bytes: Some(100 * GIB),
        ..FakeSettings::default()
    };
    assert_eq!(evaluate(&settings, Some(512 * MIB)), Verdict::Fits);
    assert_eq!(evaluate(&settings, Some(2048 * MIB)), Verdict::TooLarge);
}

#[test]
fn evaluate_negative_file_size_is_unknown() {
    // C# twin: RecycleBinCapacityGuardTests.Evaluate_NegativeSize_Unknown.
    assert_eq!(evaluate(&FakeSettings::default(), Some(-1)), Verdict::Unknown);
}

#[test]
fn evaluate_blank_path_is_rejected() {
    assert_eq!(
        RecycleBinCapacityGuard::evaluate(" \t", None, &FakeSettings::default(), None)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
}

// FIX-G30 (recycle-policy M1): the type/length of the SECOND registry query are validated as well.

const REG_DWORD: u32 = 4;
const REG_BINARY: u32 = 3;

/// Scripted two-phase registry: (kind, byte_count, data) for the probe and for the data query.
type Phase = Option<(u32, u32)>;

fn scripted(probe: Phase, read: Phase, value: u32) -> io::Result<Option<RegistryValue>> {
    read_dword_with_query(|data, count| {
        let phase = if data.is_some() { read } else { probe };
        let Some((kind, bytes)) = phase else { return Ok(None) };
        if let Some(slot) = data {
            *slot = value;
        }
        *count = bytes;
        Ok(Some(kind))
    })
}

#[test]
fn second_query_with_changed_kind_or_length_is_other() {
    let probe = Some((REG_DWORD, 4));
    for read in [
        Some((REG_BINARY, 4)),
        Some((REG_DWORD, 1)),
        Some((REG_DWORD, 0)),
        Some((REG_DWORD, 8)),
    ] {
        assert_eq!(
            scripted(probe, read, 0).unwrap(),
            Some(RegistryValue::Other),
            "{read:?}"
        );
    }
}

#[test]
fn first_query_with_wrong_kind_or_length_is_other_and_data_query_is_skipped() {
    for probe in [(REG_BINARY, 4), (REG_DWORD, 8), (REG_DWORD, 0)] {
        let mut data_calls = 0;
        let got = read_dword_with_query(|data, count| {
            if data.is_some() {
                data_calls += 1;
            }
            *count = probe.1;
            Ok(Some(probe.0))
        });
        assert_eq!(got.unwrap(), Some(RegistryValue::Other));
        assert_eq!(data_calls, 0, "no data read after a rejected probe");
    }
}

#[test]
fn matching_queries_return_the_dword_including_u32_max() {
    assert_eq!(
        scripted(Some((REG_DWORD, 4)), Some((REG_DWORD, 4)), u32::MAX).unwrap(),
        Some(RegistryValue::Dword(u32::MAX))
    );
    assert_eq!(scripted(None, None, 0).unwrap(), None);
}

#[test]
fn value_vanishing_or_failing_between_the_queries_fails_closed() {
    let gone = scripted(Some((REG_DWORD, 4)), None, 0).unwrap_err();
    assert_eq!(gone.kind(), io::ErrorKind::NotFound);
    for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::InvalidData] {
        let err = read_dword_with_query(|data, count| {
            if data.is_some() {
                return Err(io::Error::from(kind));
            }
            *count = 4;
            Ok(Some(REG_DWORD))
        })
        .unwrap_err();
        assert_eq!(err.kind(), kind);
    }
    let more_data = read_dword_with_query(|_, _| Err(io::Error::from_raw_os_error(234))).unwrap_err();
    assert_eq!(more_data.raw_os_error(), Some(234));
}

#[test]
fn changed_second_query_blocks_the_capacity_guard_through_the_settings_source() {
    let source = WindowsRecycleBinSettingsSource::with_adapters(
        |_, _, name| {
            if name == "NoRecycleFiles" {
                scripted(Some((REG_DWORD, 4)), Some((REG_BINARY, 4)), 0)
            } else {
                Ok(None)
            }
        },
        |_| Some(r"C:\".to_owned()),
        |_| Some(format!(r"\?\Volume{}\", VOLUME_GUID)),
        |_| Some(DriveType::Fixed),
        |_| Some((100 * GIB) as u64),
    );
    assert_eq!(source.read_policy().unwrap_err().kind(), io::ErrorKind::InvalidData);
    let verdict = RecycleBinCapacityGuard::evaluate(PHOTO, Some(1), &source, Some(&NullLog)).expect("valid test path");
    assert_eq!(verdict, Verdict::Unknown);
}
