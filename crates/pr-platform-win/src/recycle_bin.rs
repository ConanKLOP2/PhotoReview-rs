//! Port of PhotoReview.Platform.Windows/WindowsRecycleBin.cs and its ShellFileRecycler send path.
//!
//! Safety-sensitive operations are injected for deterministic tests. The default recycle operation delegates one
//! caller-supplied path to IFileOperation; it never enumerates, sweeps, or empties the Recycle Bin.

use std::io;
use std::path::Path;
use std::sync::Arc;

use pr_core::abstractions::{Log, NullLog, RecycleBin};
use pr_core::time::DateTimeUtc;

use crate::recycle_bin_capacity::{RecycleBinCapacityGuard, RecycleCapacityVerdict};
use crate::recycle_bin_settings::{DriveType, RecycleBinSettingsSource, WindowsRecycleBinSettingsSource};
use crate::recycle_eligibility::{can_recycle_with, is_permanent_delete_allowed_with};

type Exists = dyn Fn(&Path) -> bool + Send + Sync;
type FileLength = dyn Fn(&Path) -> io::Result<u64> + Send + Sync;
type PathAction = dyn Fn(&Path) -> io::Result<()> + Send + Sync;
type DriveTypeQuery = dyn Fn(&str) -> Option<DriveType> + Send + Sync;
type MountPointQuery = dyn Fn(&str) -> Option<String> + Send + Sync;

/// Backend seam for restoring one matching Recycle Bin item; W6-02e supplies the Shell implementation.
pub trait RecycleBinRestoreBackend: Send + Sync {
    fn try_restore(
        &self,
        original_path: &Path,
        expected_size: u64,
        expected_last_write_utc: DateTimeUtc,
    ) -> io::Result<bool>;
}

/// Windows IRecycleBin facade with read-only policy checks before any irreversible or Shell action.
pub struct WindowsRecycleBin {
    settings: Arc<dyn RecycleBinSettingsSource>,
    log: Arc<dyn Log>,
    drive_type_of: Arc<DriveTypeQuery>,
    mount_point_of: Arc<MountPointQuery>,
    file_exists: Arc<Exists>,
    file_length: Arc<FileLength>,
    shell_recycle: Arc<PathAction>,
    file_delete: Arc<PathAction>,
    restore_backend: Arc<dyn RecycleBinRestoreBackend>,
}

impl std::fmt::Debug for WindowsRecycleBin {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("WindowsRecycleBin")
    }
}

impl WindowsRecycleBin {
    /// Creates the production facade. Restore is supplied separately so its Shell backend can be added by W6-02e.
    ///
    /// Prefer [`Self::with_log`] in the application: this variant discards every "Recycle refused" warning.
    pub fn new(restore_backend: Arc<dyn RecycleBinRestoreBackend>) -> Self {
        Self::with_log(restore_backend, Arc::new(NullLog))
    }

    /// Creates the production facade writing refusals and settings-read failures to `log` (C# passes its `ILog`).
    pub fn with_log(restore_backend: Arc<dyn RecycleBinRestoreBackend>, log: Arc<dyn Log>) -> Self {
        Self::with_adapters(
            Arc::new(WindowsRecycleBinSettingsSource::default()),
            log,
            crate::recycle_bin_settings::query_drive_type,
            crate::recycle_bin_settings::query_mount_point,
            |path| path.exists(),
            |path| std::fs::metadata(path).map(|metadata| metadata.len()),
            native_shell_recycle,
            native_file_delete,
            restore_backend,
        )
    }

    /// Creates a facade with injected settings, path checks, and actions. Tests should use fakes for every action.
    #[allow(clippy::too_many_arguments)] // Each seam protects a distinct I/O or destructive boundary.
    pub fn with_adapters(
        settings: Arc<dyn RecycleBinSettingsSource>,
        log: Arc<dyn Log>,
        drive_type_of: impl Fn(&str) -> Option<DriveType> + Send + Sync + 'static,
        mount_point_of: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
        file_exists: impl Fn(&Path) -> bool + Send + Sync + 'static,
        file_length: impl Fn(&Path) -> io::Result<u64> + Send + Sync + 'static,
        shell_recycle: impl Fn(&Path) -> io::Result<()> + Send + Sync + 'static,
        file_delete: impl Fn(&Path) -> io::Result<()> + Send + Sync + 'static,
        restore_backend: Arc<dyn RecycleBinRestoreBackend>,
    ) -> Self {
        Self {
            settings,
            log,
            drive_type_of: Arc::new(drive_type_of),
            mount_point_of: Arc::new(mount_point_of),
            file_exists: Arc::new(file_exists),
            file_length: Arc::new(file_length),
            shell_recycle: Arc::new(shell_recycle),
            file_delete: Arc::new(file_delete),
            restore_backend,
        }
    }

    fn path_text(path: &Path) -> Option<&str> {
        let text = path.to_str()?;
        (!text.trim().is_empty()).then_some(text)
    }

    fn unsupported_drive(path: &Path) -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            format!("Recycle Bin is not available for {}", display_name(path)),
        )
    }

    fn cannot_hold(path: &Path, reason: &str) -> io::Error {
        io::Error::other(format!("Recycle Bin cannot hold {} ({reason})", display_name(path)))
    }

    fn not_removed(path: &Path) -> io::Error {
        io::Error::other(format!(
            "Recycle operation left the source in place: {}",
            display_name(path)
        ))
    }
}

impl RecycleBin for WindowsRecycleBin {
    fn send_to_recycle_bin(&self, path: &Path) -> io::Result<()> {
        let path_text = Self::path_text(path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path cannot be empty or non-Unicode"))?;

        if !can_recycle_with(
            path_text,
            |root| (self.drive_type_of)(root),
            |full| (self.mount_point_of)(full),
        ) {
            return Err(Self::unsupported_drive(path));
        }

        // Re-read immediately before invoking the Shell; caller preflight can be stale if the file grew.
        let current_size = match (self.file_length)(path) {
            Ok(size) => size,
            Err(error) => {
                self.log
                    .warn(&format!("Recycle refused (size unreadable): {}", display_name(path)));
                return Err(Self::cannot_hold(path, &error.to_string()));
            }
        };
        let Ok(size) = i64::try_from(current_size) else {
            self.log
                .warn(&format!("Recycle refused (size out of range): {}", display_name(path)));
            return Err(Self::cannot_hold(path, "file size is out of range"));
        };
        let verdict =
            RecycleBinCapacityGuard::evaluate(path_text, Some(size), self.settings.as_ref(), Some(self.log.as_ref()))?;
        if verdict != RecycleCapacityVerdict::Fits {
            self.log
                .warn(&format!("Recycle refused ({verdict:?}): {}", display_name(path)));
            return Err(Self::cannot_hold(path, &format!("{verdict:?}")));
        }

        (self.shell_recycle)(path)?;
        if (self.file_exists)(path) {
            return Err(Self::not_removed(path));
        }
        Ok(())
    }

    fn can_recycle(&self, path: &Path) -> bool {
        let Some(path_text) = Self::path_text(path) else {
            return false;
        };
        can_recycle_with(
            path_text,
            |root| (self.drive_type_of)(root),
            |full| (self.mount_point_of)(full),
        )
    }

    fn fits_in_recycle_bin(&self, path: &Path, file_size: u64) -> bool {
        let Some(path_text) = Self::path_text(path) else {
            return false;
        };
        let Ok(size) = i64::try_from(file_size) else {
            return false;
        };
        match RecycleBinCapacityGuard::evaluate(path_text, Some(size), self.settings.as_ref(), Some(self.log.as_ref()))
        {
            Ok(RecycleCapacityVerdict::Fits) => true,
            Ok(verdict) => {
                self.log.warn(&format!(
                    "Recycle refused ({verdict:?}): {} ({file_size} bytes)",
                    display_name(path)
                ));
                false
            }
            Err(_) => false,
        }
    }

    fn delete_permanently(&self, path: &Path) -> io::Result<()> {
        let path_text = Self::path_text(path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path cannot be empty or non-Unicode"))?;
        if !is_permanent_delete_allowed_with(
            path_text,
            |root| (self.drive_type_of)(root),
            |full| (self.mount_point_of)(full),
        ) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "permanent delete requires a positively identified no-bin volume",
            ));
        }
        (self.file_delete)(path)?;
        if (self.file_exists)(path) {
            return Err(Self::not_removed(path));
        }
        Ok(())
    }

    fn try_restore(
        &self,
        original_path: &Path,
        expected_size: u64,
        expected_last_write_utc: DateTimeUtc,
    ) -> io::Result<bool> {
        if Self::path_text(original_path).is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path cannot be empty or non-Unicode",
            ));
        }
        if (self.file_exists)(original_path) {
            self.log.warn(&format!(
                "Recycle Bin restore refused, destination already exists: {}",
                display_name(original_path)
            ));
            return Ok(false);
        }
        self.restore_backend
            .try_restore(original_path, expected_size, expected_last_write_utc)
    }
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("file")
        .to_owned()
}

/// Production permanent delete: C# `File.Delete` = `DeleteFileW` through the shared `PhysicalFileSystem::delete`
/// (already reused by the file-action path). `std::fs::remove_file` also deletes read-only files on Windows, which C#
/// refuses (`UnauthorizedAccessException`); a missing file stays success. ST-INVMAP M2, user decision Q2 2026-10-31.
pub fn native_file_delete(path: &Path) -> io::Result<()> {
    pr_core::abstractions::FileSystem::delete(&pr_core::io::physical::PhysicalFileSystem, path)
}

fn native_shell_recycle(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        shell::recycle(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows Shell recycle is available only on Windows",
        ))
    }
}

#[cfg(windows)]
mod shell {
    use super::*;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };
    use windows::Win32::UI::Shell::{
        FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOCONFIRMMKDIR, FOF_SILENT, FOFX_RECYCLEONDELETE, FileOperation,
        IFileOperation, IShellItem, SHCreateItemFromParsingName,
    };
    use windows::core::PCWSTR;

    const HR_CANCELLED: i32 = 0x8007_04c7u32 as i32;
    const HR_ABORT: i32 = 0x8000_4004u32 as i32;

    struct ComApartment;

    impl Drop for ComApartment {
        fn drop(&mut self) {
            // SAFETY: this guard exists only after CoInitializeEx succeeded on the current STA thread.
            unsafe { CoUninitialize() };
        }
    }

    pub(super) fn recycle(path: &Path) -> io::Result<()> {
        let path = std::path::absolute(path)?;
        std::thread::Builder::new()
            .name("PhotoReviewRs-RecycleShell".to_owned())
            .spawn(move || recycle_on_sta(&path))?
            .join()
            .map_err(|_| io::Error::other("Recycle Shell STA thread panicked"))?
    }

    fn recycle_on_sta(path: &Path) -> io::Result<()> {
        // SAFETY: this worker thread is dedicated to one Shell operation and has no prior COM apartment.
        let status = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if !status.is_ok() {
            return Err(io::Error::other(format!(
                "COM STA initialization failed ({:#010x})",
                status.0 as u32
            )));
        }
        let _apartment = ComApartment;

        let text = path
            .to_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path is not valid Unicode"))?;
        if text.contains('\0') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path contains a NUL character",
            ));
        }
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: wide is a live NUL-terminated path string; the returned interface owns its COM reference.
        let item: IShellItem = unsafe { SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(wide.as_ptr()), None) }
            .map_err(|error| map_shell_error(error.code().0, path))?;
        // SAFETY: CLSID_FileOperation is registered and the requested interface is IFileOperation.
        let operation: IFileOperation = unsafe { CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER) }
            .map_err(|error| map_shell_error(error.code().0, path))?;
        let flags = FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOCONFIRMMKDIR | FOF_SILENT | FOFX_RECYCLEONDELETE;
        // SAFETY: the operation and shell item are live COM interfaces used only on their creating STA thread.
        check_shell_call(unsafe { operation.SetOperationFlags(flags) }, path)?;
        // SAFETY: the operation and item are live COM interfaces; None requests no progress callback.
        check_shell_call(unsafe { operation.DeleteItem(&item, None) }, path)?;
        // SAFETY: queued work contains only the one caller-supplied item.
        check_shell_call(unsafe { operation.PerformOperations() }, path)?;
        Ok(())
    }

    fn check_shell_call(result: windows::core::Result<()>, path: &Path) -> io::Result<()> {
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                let code = error.code().0;
                if code == HR_CANCELLED || code == HR_ABORT {
                    // Match C# ShellFileRecycler.Throw: cancellation is checked by the facade's source-exists guard.
                    Ok(())
                } else {
                    Err(map_shell_error(code, path))
                }
            }
        }
    }

    fn map_shell_error(code: i32, path: &Path) -> io::Error {
        let kind = match code as u32 {
            0x8007_0002 | 0x8007_0003 => io::ErrorKind::NotFound,
            0x8007_0005 => io::ErrorKind::PermissionDenied,
            0x8007_00ce => io::ErrorKind::InvalidInput,
            _ => io::ErrorKind::Other,
        };
        io::Error::new(
            kind,
            format!("IFileOperation failed ({code:#010x}): {}", display_name(path)),
        )
    }
}
