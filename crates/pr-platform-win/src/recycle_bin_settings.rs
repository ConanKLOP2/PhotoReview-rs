//! Port of `PhotoReview.Platform.Windows/RecycleBinCapacity.cs` settings source and shared volume-query adapter.

use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Drive classification returned by `GetDriveTypeW` for a volume root or mount point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveType {
    Unknown,
    NoRootDirectory,
    Removable,
    Fixed,
    Network,
    CdRom,
    Ram,
}

/// Values from `HKCU\...\Explorer\BitBucket\Volume\{guid}`; `None` means a value is absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RecycleBinVolumeSettings {
    pub nuke_on_delete: Option<i64>,
    pub max_capacity_mb: Option<i64>,
}

/// User and machine policy switches affecting Recycle Bin capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RecycleBinPolicy {
    pub no_recycle_files: bool,
    pub max_size_percent: Option<i64>,
    pub legacy_global_nuke_on_delete: bool,
}

/// Registry hives queried by the read-only Windows settings adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegistryHive {
    CurrentUser,
    LocalMachine,
}

/// A registry value type sufficient to preserve C# `RegistryKey.GetValue` DWORD validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryValue {
    Dword(u32),
    Other,
}

/// Reads Recycle Bin policy and per-volume registry data and resolves volume metadata.
pub trait RecycleBinSettingsSource: Send + Sync {
    fn read_policy(&self) -> io::Result<RecycleBinPolicy>;
    fn get_volume_guid(&self, path: &str) -> Option<String>;
    fn get_volume_drive_type(&self, path: &str) -> Option<DriveType>;
    fn read_volume(&self, volume_guid: &str) -> io::Result<RecycleBinVolumeSettings>;
    fn get_user_quota_bytes(&self, path: &str) -> Option<i64>;
}

type RegistryReader = dyn Fn(RegistryHive, &str, &str) -> io::Result<Option<RegistryValue>> + Send + Sync;
type MountPointReader = dyn Fn(&str) -> Option<String> + Send + Sync;
type VolumeNameReader = dyn Fn(&str) -> Option<String> + Send + Sync;
type DriveTypeReader = dyn Fn(&str) -> Option<DriveType> + Send + Sync;
type TotalBytesReader = dyn Fn(&str) -> Option<u64> + Send + Sync;

/// Native read-only Windows registry and volume metadata source.
pub struct WindowsRecycleBinSettingsSource {
    read_value: Arc<RegistryReader>,
    mount_point: Arc<MountPointReader>,
    volume_name: Arc<VolumeNameReader>,
    drive_type: Arc<DriveTypeReader>,
    total_bytes: Arc<TotalBytesReader>,
}

impl std::fmt::Debug for WindowsRecycleBinSettingsSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("WindowsRecycleBinSettingsSource")
    }
}

impl Default for WindowsRecycleBinSettingsSource {
    fn default() -> Self {
        Self::with_reader(read_registry_value)
    }
}

impl WindowsRecycleBinSettingsSource {
    const POLICY_KEY: &'static str = "Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer";
    const BIT_BUCKET_KEY: &'static str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\BitBucket";

    /// Builds a settings source using an injected read-only registry reader.
    pub fn with_reader(
        reader: impl Fn(RegistryHive, &str, &str) -> io::Result<Option<RegistryValue>> + Send + Sync + 'static,
    ) -> Self {
        Self {
            read_value: Arc::new(reader),
            mount_point: Arc::new(query_mount_point),
            volume_name: Arc::new(query_volume_name),
            drive_type: Arc::new(query_drive_type),
            total_bytes: Arc::new(query_total_bytes),
        }
    }

    /// Builds a source with injected registry, mount-point, volume-name, drive-type, and quota readers.
    pub fn with_adapters(
        reader: impl Fn(RegistryHive, &str, &str) -> io::Result<Option<RegistryValue>> + Send + Sync + 'static,
        mount_point: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
        volume_name: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
        drive_type: impl Fn(&str) -> Option<DriveType> + Send + Sync + 'static,
        total_bytes: impl Fn(&str) -> Option<u64> + Send + Sync + 'static,
    ) -> Self {
        Self {
            read_value: Arc::new(reader),
            mount_point: Arc::new(mount_point),
            volume_name: Arc::new(volume_name),
            drive_type: Arc::new(drive_type),
            total_bytes: Arc::new(total_bytes),
        }
    }

    fn read_dword(&self, hive: RegistryHive, key: &str, name: &str) -> io::Result<Option<i64>> {
        match (self.read_value)(hive, key, name)? {
            None => Ok(None),
            Some(RegistryValue::Dword(value)) => Ok(Some(i64::from(value))),
            Some(RegistryValue::Other) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Registry value {key}\\{name} is not a DWORD."),
            )),
        }
    }

    /// Extracts the brace-form GUID from a `\\?\Volume{...}\` name.
    pub fn extract_guid(volume_name: &str) -> Option<String> {
        const PREFIX: &str = "\\\\?\\Volume{";
        if !volume_name
            .get(..PREFIX.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(PREFIX))
        {
            return None;
        }
        let close = volume_name[PREFIX.len()..].find('}')? + PREFIX.len();
        let guid = &volume_name[PREFIX.len() - 1..=close];
        if guid.len() != 38
            || guid.as_bytes()[0] != b'{'
            || guid.as_bytes()[37] != b'}'
            || ![9, 14, 19, 24].iter().all(|&index| guid.as_bytes()[index] == b'-')
            || !guid
                .bytes()
                .enumerate()
                .all(|(index, byte)| matches!(index, 0 | 9 | 14 | 19 | 24 | 37) || byte.is_ascii_hexdigit())
        {
            return None;
        }
        Some(guid.to_owned())
    }

    fn mount_point(&self, path: &str) -> Option<String> {
        (self.mount_point)(path)
    }
}

impl RecycleBinSettingsSource for WindowsRecycleBinSettingsSource {
    fn read_policy(&self) -> io::Result<RecycleBinPolicy> {
        let no_recycle_files =
            nonzero(self.read_dword(RegistryHive::CurrentUser, Self::POLICY_KEY, "NoRecycleFiles")?)
                || nonzero(self.read_dword(RegistryHive::LocalMachine, Self::POLICY_KEY, "NoRecycleFiles")?);
        let user_percent = self.read_dword(RegistryHive::CurrentUser, Self::POLICY_KEY, "RecycleBinSize")?;
        let machine_percent = self.read_dword(RegistryHive::LocalMachine, Self::POLICY_KEY, "RecycleBinSize")?;
        let max_size_percent = match (user_percent, machine_percent) {
            (Some(user), Some(machine)) => Some(user.min(machine)),
            (Some(user), None) => Some(user),
            (None, Some(machine)) => Some(machine),
            (None, None) => None,
        };
        let legacy_global_nuke_on_delete =
            nonzero(self.read_dword(RegistryHive::CurrentUser, Self::BIT_BUCKET_KEY, "NukeOnDelete")?);
        Ok(RecycleBinPolicy {
            no_recycle_files,
            max_size_percent,
            legacy_global_nuke_on_delete,
        })
    }

    fn get_volume_guid(&self, path: &str) -> Option<String> {
        let mount_point = self.mount_point(path)?;
        let name = (self.volume_name)(&mount_point)?;
        Self::extract_guid(&name)
    }

    fn get_volume_drive_type(&self, path: &str) -> Option<DriveType> {
        self.mount_point(path)
            .and_then(|mount_point| (self.drive_type)(&mount_point))
    }

    fn read_volume(&self, volume_guid: &str) -> io::Result<RecycleBinVolumeSettings> {
        if volume_guid.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "volume GUID cannot be empty or whitespace",
            ));
        }
        let key = format!("{}\\Volume\\{volume_guid}", Self::BIT_BUCKET_KEY);
        Ok(RecycleBinVolumeSettings {
            nuke_on_delete: self.read_dword(RegistryHive::CurrentUser, &key, "NukeOnDelete")?,
            max_capacity_mb: self.read_dword(RegistryHive::CurrentUser, &key, "MaxCapacity")?,
        })
    }

    fn get_user_quota_bytes(&self, path: &str) -> Option<i64> {
        let mount_point = self.mount_point(path)?;
        let total = (self.total_bytes)(&mount_point)?;
        if total == 0 || total > i64::MAX as u64 {
            None
        } else {
            Some(total as i64)
        }
    }
}

fn nonzero(value: Option<i64>) -> bool {
    value.is_some_and(|value| value != 0)
}

/// Resolves the mount point that actually contains `path`; follows junctions and mounted folders.
pub fn query_mount_point(path: &str) -> Option<String> {
    if path.trim().is_empty() || path.contains('\0') {
        return None;
    }
    let full = absolute_normalized(Path::new(path))?;
    #[cfg(windows)]
    {
        use windows::Win32::Storage::FileSystem::GetVolumePathNameW;
        use windows::core::PCWSTR;
        let full_wide = to_wide(&full)?;
        let capacity = full.encode_utf16().count().saturating_add(2).clamp(262, 32_768);
        let mut buffer = vec![0u16; capacity];
        // SAFETY: full_wide is NUL terminated and buffer has the C#-matching bounded capacity.
        unsafe { GetVolumePathNameW(PCWSTR(full_wide.as_ptr()), &mut buffer) }.ok()?;
        let length = buffer.iter().position(|&unit| unit == 0).unwrap_or(buffer.len());
        let mount = String::from_utf16(&buffer[..length]).ok()?;
        (!mount.is_empty()).then_some(mount)
    }
    #[cfg(not(windows))]
    {
        let _ = full;
        None
    }
}

fn query_volume_name(mount_point: &str) -> Option<String> {
    #[cfg(windows)]
    {
        use windows::Win32::Storage::FileSystem::GetVolumeNameForVolumeMountPointW;
        use windows::core::PCWSTR;
        let mount_wide = to_wide(mount_point)?;
        let mut buffer = [0u16; 64];
        // SAFETY: mount_wide is NUL terminated and buffer is writable for 64 UTF-16 code units.
        unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR(mount_wide.as_ptr()), &mut buffer) }.ok()?;
        let length = buffer.iter().position(|&unit| unit == 0).unwrap_or(buffer.len());
        String::from_utf16(&buffer[..length]).ok()
    }
    #[cfg(not(windows))]
    {
        let _ = mount_point;
        None
    }
}

fn query_total_bytes(mount_point: &str) -> Option<u64> {
    #[cfg(windows)]
    {
        use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        use windows::core::PCWSTR;
        let mount_wide = to_wide(mount_point)?;
        let mut total = 0u64;
        // SAFETY: mount_wide is a live NUL-terminated path and total is a valid output pointer.
        unsafe { GetDiskFreeSpaceExW(PCWSTR(mount_wide.as_ptr()), None, Some(&mut total), None) }
            .ok()
            .map(|()| total)
    }
    #[cfg(not(windows))]
    {
        let _ = mount_point;
        None
    }
}

/// Queries Windows `GetDriveTypeW` for a volume root or mount-point directory.
pub fn query_drive_type(root: &str) -> Option<DriveType> {
    if root.contains('\0') {
        return None;
    }
    let mut root = root.to_owned();
    if !root.ends_with('\\') {
        root.push('\\');
    }
    #[cfg(windows)]
    {
        use windows::Win32::Storage::FileSystem::GetDriveTypeW;
        use windows::core::PCWSTR;
        let wide = to_wide(&root)?;
        // SAFETY: wide is a live NUL-terminated root path; GetDriveTypeW does not retain it.
        Some(match unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) } {
            0 => DriveType::Unknown,
            1 => DriveType::NoRootDirectory,
            2 => DriveType::Removable,
            3 => DriveType::Fixed,
            4 => DriveType::Network,
            5 => DriveType::CdRom,
            6 => DriveType::Ram,
            _ => DriveType::Unknown,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = root;
        None
    }
}

fn absolute_normalized(path: &Path) -> Option<String> {
    let absolute = std::path::absolute(path).ok()?;
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
    Some(normalized.to_string_lossy().into_owned())
}

#[cfg(windows)]
fn to_wide(value: &str) -> Option<Vec<u16>> {
    if value.contains('\0') {
        return None;
    }
    Some(value.encode_utf16().chain(std::iter::once(0)).collect())
}

#[cfg(windows)]
fn read_registry_value(hive: RegistryHive, key: &str, name: &str) -> io::Result<Option<RegistryValue>> {
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, REG_DWORD, RegOpenKeyExW, RegQueryValueExW,
    };
    use windows::core::PCWSTR;

    let root = match hive {
        RegistryHive::CurrentUser => HKEY_CURRENT_USER,
        RegistryHive::LocalMachine => HKEY_LOCAL_MACHINE,
    };
    let key_name =
        to_wide(key).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "registry key contains NUL"))?;
    let mut opened = Default::default();
    // SAFETY: key_name is NUL terminated and opened is a valid output for a read-only registry handle.
    let status = unsafe { RegOpenKeyExW(root, PCWSTR(key_name.as_ptr()), None, KEY_QUERY_VALUE, &mut opened) };
    if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
        return Ok(None);
    }
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status.0 as i32));
    }
    struct RegistryKey(windows::Win32::System::Registry::HKEY);
    impl Drop for RegistryKey {
        fn drop(&mut self) {
            // SAFETY: this wrapper exclusively owns an opened registry key handle and closes it once.
            let _ = unsafe { windows::Win32::System::Registry::RegCloseKey(self.0) };
        }
    }
    let _key = RegistryKey(opened);
    let value_name =
        to_wide(name).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "registry value contains NUL"))?;
    let mut kind = Default::default();
    let mut byte_count = 0u32;
    // SAFETY: null data asks RegQueryValueExW to return only the type and required byte count.
    let status = unsafe {
        RegQueryValueExW(
            opened,
            PCWSTR(value_name.as_ptr()),
            None,
            Some(&mut kind),
            None,
            Some(&mut byte_count),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status.0 as i32));
    }
    if kind != REG_DWORD || byte_count != 4 {
        return Ok(Some(RegistryValue::Other));
    }
    let mut value = 0u32;
    // SAFETY: value points to exactly four writable bytes and byte_count is their capacity.
    let status = unsafe {
        RegQueryValueExW(
            opened,
            PCWSTR(value_name.as_ptr()),
            None,
            Some(&mut kind),
            Some((&mut value as *mut u32).cast()),
            Some(&mut byte_count),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status.0 as i32));
    }
    Ok(Some(RegistryValue::Dword(value)))
}

#[cfg(not(windows))]
fn read_registry_value(_hive: RegistryHive, _key: &str, _name: &str) -> io::Result<Option<RegistryValue>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows registry is unavailable",
    ))
}
