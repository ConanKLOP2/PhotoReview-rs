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
type ShellRecycleAction = dyn Fn(&Path) -> io::Result<ShellRecycleOutcome> + Send + Sync;
type DriveTypeQuery = dyn Fn(&str) -> Option<DriveType> + Send + Sync;
type MountPointQuery = dyn Fn(&str) -> Option<String> + Send + Sync;

/// Longest path (UTF-16 units, without the terminating NUL) the Shell delete is trusted with: `MAX_PATH` is 260 and
/// includes the NUL. Longer paths are refused before the Shell call because `IFileOperation` may then delete for good
/// instead of recycling (ST-COM-RECYCLE C2; the Recycle Bin cannot hold a path that long).
pub const MAX_SHELL_RECYCLE_PATH_UNITS: usize = 259;

/// What the Shell reported for the one item of a recycle request (ST-COM-RECYCLE C2). Before this the facade only
/// knew "the file is gone", which is also true after a permanent delete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellRecycleOutcome {
    /// `PostDeleteItem` succeeded and returned a new item (the one now in the Recycle Bin).
    MovedToRecycleBin,
    /// `PostDeleteItem` succeeded but returned no new item: the Shell deleted the file permanently. Data is gone.
    DeletedPermanently,
    /// `PostDeleteItem` reported a failed HRESULT (raw value).
    Failed(i32),
    /// The Shell gave no per-item report at all (the sink was never called): nothing proves the file was recycled.
    Unknown,
}

/// Pure decision of the progress sink: a failed HRESULT is `Failed`, a success with a new item is a recycle, a
/// success without one is a permanent delete.
pub fn classify_post_delete(hr_delete: i32, newly_created_present: bool) -> ShellRecycleOutcome {
    if hr_delete < 0 {
        ShellRecycleOutcome::Failed(hr_delete)
    } else if newly_created_present {
        ShellRecycleOutcome::MovedToRecycleBin
    } else {
        ShellRecycleOutcome::DeletedPermanently
    }
}

/// Keeps the worst report when the Shell calls the sink more than once for the same item (per-item sink and the
/// operation-wide sink both see the delete).
pub fn merge_outcome(existing: Option<ShellRecycleOutcome>, new: ShellRecycleOutcome) -> ShellRecycleOutcome {
    match existing {
        None | Some(ShellRecycleOutcome::MovedToRecycleBin) => new,
        Some(worse) => worse,
    }
}

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
    shell_recycle: Arc<ShellRecycleAction>,
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
        shell_recycle: impl Fn(&Path) -> io::Result<ShellRecycleOutcome> + Send + Sync + 'static,
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

    fn path_too_long(path: &Path) -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "Recycle Bin cannot hold {} (path is longer than {MAX_SHELL_RECYCLE_PATH_UNITS} characters)",
                display_name(path)
            ),
        )
    }

    fn permanently_deleted(path: &Path) -> io::Error {
        io::Error::other(format!(
            "Recycle failed: the Shell deleted {} permanently instead of moving it to the Recycle Bin",
            display_name(path)
        ))
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

        // Over MAX_PATH the Shell may delete for good instead of recycling and nothing would tell us (C2).
        if path_text.encode_utf16().count() > MAX_SHELL_RECYCLE_PATH_UNITS {
            self.log
                .warn(&format!("Recycle refused (path too long): {}", display_name(path)));
            return Err(Self::path_too_long(path));
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

        // Fail closed (C2): only a positive "moved to the Recycle Bin" report plus a vanished source counts as success,
        // so the journal never says Recycle for a file the Shell deleted for good or could not confirm.
        match (self.shell_recycle)(path)? {
            ShellRecycleOutcome::MovedToRecycleBin => {
                if (self.file_exists)(path) {
                    return Err(Self::not_removed(path));
                }
                Ok(())
            }
            ShellRecycleOutcome::DeletedPermanently => {
                self.log.warn(&format!(
                    "Recycle failed: Shell deleted permanently (no Recycle Bin item): {}",
                    display_name(path)
                ));
                Err(Self::permanently_deleted(path))
            }
            ShellRecycleOutcome::Failed(hr) => {
                if (self.file_exists)(path) {
                    return Err(Self::not_removed(path));
                }
                Err(io::Error::other(format!(
                    "Recycle failed: IFileOperation reported {:#010x} for {}",
                    hr as u32,
                    display_name(path)
                )))
            }
            ShellRecycleOutcome::Unknown => {
                if (self.file_exists)(path) {
                    return Err(Self::not_removed(path));
                }
                self.log.warn(&format!(
                    "Recycle unconfirmed: the Shell gave no per-item report: {}",
                    display_name(path)
                ));
                Err(io::Error::other(format!(
                    "Recycle unconfirmed: the Shell did not report that {} reached the Recycle Bin",
                    display_name(path)
                )))
            }
        }
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

fn native_shell_recycle(path: &Path) -> io::Result<ShellRecycleOutcome> {
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
pub mod delete_sink {
    //! The `IFileOperationProgressSink` that tells a recycle from a permanent delete (ST-COM-RECYCLE C2). Only
    //! `PostDeleteItem` matters; the other eleven callbacks return `S_OK`. The object lives on the one STA thread
    //! that created it, so a `Cell` behind an `Rc` is enough.

    use std::cell::Cell;
    use std::rc::Rc;

    use windows::Win32::UI::Shell::{IFileOperationProgressSink, IFileOperationProgressSink_Impl, IShellItem};
    use windows_core::{HRESULT, PCWSTR, Ref, implement};

    use super::{ShellRecycleOutcome, classify_post_delete, merge_outcome};

    /// Shared slot the sink writes and the recycle code reads after `PerformOperations`.
    pub type OutcomeSlot = Rc<Cell<Option<ShellRecycleOutcome>>>;

    #[implement(IFileOperationProgressSink)]
    struct DeleteSink {
        slot: OutcomeSlot,
    }

    /// Creates the sink interface and the slot it reports into.
    pub fn new_sink() -> (IFileOperationProgressSink, OutcomeSlot) {
        let slot: OutcomeSlot = Rc::new(Cell::new(None));
        let sink: IFileOperationProgressSink = DeleteSink { slot: Rc::clone(&slot) }.into();
        (sink, slot)
    }

    impl IFileOperationProgressSink_Impl for DeleteSink_Impl {
        fn StartOperations(&self) -> windows_core::Result<()> {
            Ok(())
        }
        fn FinishOperations(&self, _hrresult: HRESULT) -> windows_core::Result<()> {
            Ok(())
        }
        fn PreRenameItem(
            &self,
            _dwflags: u32,
            _psiitem: Ref<IShellItem>,
            _psznewname: &PCWSTR,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn PostRenameItem(
            &self,
            _dwflags: u32,
            _psiitem: Ref<IShellItem>,
            _psznewname: &PCWSTR,
            _hrrename: HRESULT,
            _psinewlycreated: Ref<IShellItem>,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn PreMoveItem(
            &self,
            _dwflags: u32,
            _psiitem: Ref<IShellItem>,
            _psidestinationfolder: Ref<IShellItem>,
            _psznewname: &PCWSTR,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn PostMoveItem(
            &self,
            _dwflags: u32,
            _psiitem: Ref<IShellItem>,
            _psidestinationfolder: Ref<IShellItem>,
            _psznewname: &PCWSTR,
            _hrmove: HRESULT,
            _psinewlycreated: Ref<IShellItem>,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn PreCopyItem(
            &self,
            _dwflags: u32,
            _psiitem: Ref<IShellItem>,
            _psidestinationfolder: Ref<IShellItem>,
            _psznewname: &PCWSTR,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn PostCopyItem(
            &self,
            _dwflags: u32,
            _psiitem: Ref<IShellItem>,
            _psidestinationfolder: Ref<IShellItem>,
            _psznewname: &PCWSTR,
            _hrcopy: HRESULT,
            _psinewlycreated: Ref<IShellItem>,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn PreDeleteItem(&self, _dwflags: u32, _psiitem: Ref<IShellItem>) -> windows_core::Result<()> {
            Ok(())
        }
        fn PostDeleteItem(
            &self,
            _dwflags: u32,
            _psiitem: Ref<IShellItem>,
            hrdelete: HRESULT,
            psinewlycreated: Ref<IShellItem>,
        ) -> windows_core::Result<()> {
            let new = classify_post_delete(hrdelete.0, psinewlycreated.is_some());
            self.slot.set(Some(merge_outcome(self.slot.get(), new)));
            Ok(())
        }
        fn PreNewItem(
            &self,
            _dwflags: u32,
            _psidestinationfolder: Ref<IShellItem>,
            _psznewname: &PCWSTR,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn PostNewItem(
            &self,
            _dwflags: u32,
            _psidestinationfolder: Ref<IShellItem>,
            _psznewname: &PCWSTR,
            _psztemplatename: &PCWSTR,
            _dwfileattributes: u32,
            _hrnew: HRESULT,
            _psinewitem: Ref<IShellItem>,
        ) -> windows_core::Result<()> {
            Ok(())
        }
        fn UpdateProgress(&self, _iworktotal: u32, _iworksofar: u32) -> windows_core::Result<()> {
            Ok(())
        }
        fn ResetTimer(&self) -> windows_core::Result<()> {
            Ok(())
        }
        fn PauseTimer(&self) -> windows_core::Result<()> {
            Ok(())
        }
        fn ResumeTimer(&self) -> windows_core::Result<()> {
            Ok(())
        }
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

    pub(super) fn recycle(path: &Path) -> io::Result<ShellRecycleOutcome> {
        let path = std::path::absolute(path)?;
        std::thread::Builder::new()
            .name("PhotoReviewRs-RecycleShell".to_owned())
            .spawn(move || recycle_on_sta(&path))?
            .join()
            .map_err(|_| io::Error::other("Recycle Shell STA thread panicked"))?
    }

    fn recycle_on_sta(path: &Path) -> io::Result<ShellRecycleOutcome> {
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
        let (sink, slot) = super::delete_sink::new_sink();
        // SAFETY: the operation and sink are live COM interfaces on this STA thread; the cookie is released below.
        let cookie = unsafe { operation.Advise(&sink) }.ok();
        // SAFETY: the operation, item and sink are live COM interfaces; the item sink also reports PostDeleteItem.
        let queued = check_shell_call(unsafe { operation.DeleteItem(&item, &sink) }, path);
        // SAFETY: queued work contains only the one caller-supplied item.
        let performed = queued.and_then(|()| check_shell_call(unsafe { operation.PerformOperations() }, path));
        if let Some(cookie) = cookie {
            // SAFETY: cookie came from the Advise call above on the same operation.
            let _ = unsafe { operation.Unadvise(cookie) };
        }
        performed?;
        Ok(slot.get().unwrap_or(ShellRecycleOutcome::Unknown))
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
