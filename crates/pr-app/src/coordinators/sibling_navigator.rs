//! Port of `PhotoReview.App.Shared/Coordinators/SiblingFolderNavigator.cs` (130 lines, baseline `d92d6aae`):
//! PageUp / PageDown to the previous / next sibling folder that holds a supported image, and the same search for the
//! info overlay (`FindSiblingImageFolders`). Contract: `contracts/W8-06f.md`.
//!
//! Decisions (see the contract):
//! - D-W8-2: C# `NavigateSiblingFolderAsync` (read state, `Task.Run` the search, check the folder generation, then status or
//!   open) becomes three calls with the same order: [`SiblingFolderNavigator::begin_navigate`] on the UI thread,
//!   [`SiblingFolderNavigator::find_next_image_folder`] on a worker (blocking directory I/O), and
//!   [`SiblingFolderNavigator::complete_navigate`] back on the UI thread. The generation is read in `begin_navigate`
//!   and compared in `complete_navigate` (INV-5 / INV-9 style folder generation).
//! - D-W8-1: the navigator holds no catalog, session or sink. The caller passes the current session folder and the current
//!   catalog path to `begin_navigate`, and the sink to `complete_navigate` (no `Rc<RefCell>` graph).
//! - D-W8-4: the boundary text comes from `viewmodels::format::status::sibling_folder_boundary`.
//! - `SiblingImageFolders` is NOT defined here: it lives in `viewmodels::info_overlay` (W8-06b), which needed it first.
//! - The sorted sibling list is `pr_core::catalog::sorting::get_sorted_siblings` (C# `SiblingFolderService.GetSorted`,
//!   already ported in W2-05, it reads the real disk); the per-sibling image probe goes through `FileSystem::enumerate_files`.
//!
//! Pure module: no `unsafe`, no `windows` crate. `find_next_image_folder` and `find_sibling_image_folders` BLOCK on directory
//! I/O: never call them on the UI thread.

use std::fmt;
use std::sync::Arc;

use std::path::Path;

use pr_core::abstractions::FileSystem;
use pr_core::catalog::generation::GenerationClock;
use pr_core::catalog::sorting::get_sorted_siblings;
use pr_core::catalog::types::is_supported_with;
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::{PathError, get_directory_name, get_full_path, trim_ending_directory_separator};
use pr_core::text::eq_ignore_case;

use crate::coordinators::traits::ISiblingNavigatorSink;
use crate::viewmodels::format::context::FormatContext;
use crate::viewmodels::format::status::sibling_folder_boundary;
use crate::viewmodels::info_overlay::SiblingImageFolders;

/// C# `Func<bool>` setting getter (`getRawEnabled`, `getWebpHeicEnabled`). Read inside the search, on whichever thread runs it,
/// as in C# (`Task.Run`); an absent getter means `false`.
pub type SettingGetter = Arc<dyn Fn() -> bool + Send + Sync>;

/// Why a navigation or a sibling search did not produce a result. Replaces the C# exceptions that escape the task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiblingNavError {
    /// C# `ArgumentOutOfRangeException(nameof(direction))`: the direction is not -1 or +1 (RV-A08).
    InvalidDirection(i32),
    /// C# `ArgumentException.ThrowIfNullOrWhiteSpace(currentFolder)` in `FindSiblingImageFolders`.
    BlankFolder,
    /// C# `Path.GetFullPath` rejected the folder.
    InvalidPath(PathError),
    /// C# `OperationCanceledException` from the token of `find_sibling_image_folders`.
    Cancelled,
}

impl fmt::Display for SiblingNavError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDirection(d) => write!(f, "direction must be -1 or 1, got {d}"),
            Self::BlankFolder => f.write_str("the folder must not be empty or blank"),
            Self::InvalidPath(e) => write!(f, "invalid folder path: {e:?}"),
            Self::Cancelled => f.write_str("the sibling folder search was cancelled"),
        }
    }
}

impl std::error::Error for SiblingNavError {}

/// The search a navigation needs (what `begin_navigate` captured on the UI thread). Plain data, `Send`: it goes to the worker
/// and comes back to `complete_navigate` unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiblingSearch {
    /// C# `currentFolder`: `Path.GetFullPath(folder)` of the session folder, or of the directory of the current catalog entry.
    pub current_folder: String,
    /// -1 (previous) or +1 (next).
    pub direction: i32,
    /// C# `folderGeneration = _clock.CurrentFolder`, read before the worker starts.
    pub folder_generation: i64,
}

/// What `complete_navigate` did (the C# method returns nothing; tests need it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiblingNavOutcome {
    /// The folder generation changed while the search ran (the user opened another folder): no status, no open.
    Stale,
    /// No sibling image folder in that direction: the boundary status was set.
    Boundary,
    /// `sink.open_folder(target, None)` was requested.
    Opened,
}

/// C# `SiblingFolderNavigator`. Cheap to share (`Arc`), `Send + Sync`: the worker uses the same instance.
pub struct SiblingFolderNavigator {
    clock: Arc<GenerationClock>,
    file_system: Arc<dyn FileSystem>,
    get_raw_enabled: Option<SettingGetter>,
    get_webp_heic_enabled: Option<SettingGetter>,
}

impl SiblingFolderNavigator {
    /// C# constructor without `catalog`, `sink` and `getCurrentSession` (they are per-call arguments here, see the module doc).
    /// The C# null checks have no Rust form (the types cannot be null).
    pub fn new(
        clock: Arc<GenerationClock>,
        file_system: Arc<dyn FileSystem>,
        get_raw_enabled: Option<SettingGetter>,
        get_webp_heic_enabled: Option<SettingGetter>,
    ) -> Self {
        Self {
            clock,
            file_system,
            get_raw_enabled,
            get_webp_heic_enabled,
        }
    }

    /// C# `NavigateSiblingFolderAsync` up to `Task.Run`, UI thread. In this order:
    /// 1. `direction` not -1 or +1: `Err(InvalidDirection)`, before anything else is read (RV-A08: direction 0 never advanced the loop).
    /// 2. `folder = session_folder`, or, when there is NO session (`None`), the directory of `catalog_current_path`
    ///    (`pr_core::text::dotnet_path::get_directory_name`, `None` for a drive root or no current entry). A session whose folder
    ///    text is empty does NOT fall back to the catalog (the C# null-coalescing operator tests for null only).
    /// 3. A `None` or whitespace-only folder gives `Ok(None)` (a no-op).
    /// 4. `current_folder = get_full_path(folder)` (`Err(InvalidPath)` on a bad path), `folder_generation = clock.current_folder()`.
    pub fn begin_navigate(
        &self,
        direction: i32,
        session_folder: Option<&str>,
        catalog_current_path: Option<&str>,
    ) -> Result<Option<SiblingSearch>, SiblingNavError> {
        if direction != -1 && direction != 1 {
            return Err(SiblingNavError::InvalidDirection(direction));
        }
        let folder: Option<String> = match session_folder {
            Some(f) => Some(f.to_owned()),
            None => catalog_current_path.and_then(get_directory_name),
        };
        let Some(folder) = folder else {
            return Ok(None);
        };
        if folder.trim().is_empty() {
            return Ok(None);
        }
        let current_folder = get_full_path(&folder).map_err(SiblingNavError::InvalidPath)?;
        Ok(Some(SiblingSearch {
            current_folder,
            direction,
            folder_generation: self.clock.current_folder(),
        }))
    }

    /// C# `FindNextImageFolder`, WORKER thread, blocking: `get_sorted_siblings(current_folder, None)`, the index of the current
    /// folder in it, then the nearest sibling in `direction` that holds a supported image. `Ok(None)` when the sorted list is
    /// empty (a root folder or an unreadable parent), when the current folder is not in it, or when no sibling qualifies.
    /// `Err(InvalidPath)` for a bad path, `Err(InvalidDirection)` for a direction other than +-1.
    pub fn find_next_image_folder(&self, search: &SiblingSearch) -> Result<Option<String>, SiblingNavError> {
        let folders = get_sorted_siblings(&search.current_folder, None).map_err(SiblingNavError::InvalidPath)?;
        let index = Self::index_of_folder(&folders, &search.current_folder).map_err(SiblingNavError::InvalidPath)?;
        match index {
            None => Ok(None),
            Some(index) => self.find_image_folder(&folders, index, search.direction, &CancelToken::none()),
        }
    }

    /// The code after `await Task.Run(..)`, UI thread:
    /// 1. `!clock.is_folder_current(search.folder_generation)`: `Stale`, nothing else (not even a status).
    /// 2. `target == None`: `sink.set_status_text(sibling_folder_boundary(search.direction, ctx))`, `Boundary`.
    /// 3. Else `sink.open_folder(&target, None)` once (the request is posted, D-W8-2), `Opened`.
    pub fn complete_navigate(
        &self,
        search: &SiblingSearch,
        target: Option<&str>,
        sink: &mut dyn ISiblingNavigatorSink,
        ctx: &FormatContext<'_>,
    ) -> SiblingNavOutcome {
        if !self.clock.is_folder_current(search.folder_generation) {
            return SiblingNavOutcome::Stale;
        }
        match target {
            None => {
                sink.set_status_text(&sibling_folder_boundary(search.direction, ctx));
                SiblingNavOutcome::Boundary
            }
            Some(target) => {
                sink.open_folder(target, None);
                SiblingNavOutcome::Opened
            }
        }
    }

    /// C# `FindSiblingImageFolders(currentFolder, cancellationToken)`, blocking: the folders `begin_navigate` would reach with
    /// -1 and +1 (the info overlay and the keys never disagree). Order, as C#:
    /// 1. blank `current_folder`: `Err(BlankFolder)`; `full = get_full_path(current_folder)` (`Err(InvalidPath)`);
    /// 2. `folders = get_sorted_siblings(full, None)`;
    /// 3. `cancel.check()` (`Err(Cancelled)`), AFTER the listing;
    /// 4. the current folder not in the list: `Ok(SiblingImageFolders::default())`;
    /// 5. previous = search with -1, next = search with +1; the token is checked before every sibling probed.
    pub fn find_sibling_image_folders(
        &self,
        current_folder: &str,
        cancel: &CancelToken,
    ) -> Result<SiblingImageFolders, SiblingNavError> {
        if current_folder.trim().is_empty() {
            return Err(SiblingNavError::BlankFolder);
        }
        let full = get_full_path(current_folder).map_err(SiblingNavError::InvalidPath)?;
        let folders = get_sorted_siblings(&full, None).map_err(SiblingNavError::InvalidPath)?;
        cancel.check().map_err(|_| SiblingNavError::Cancelled)?;
        let Some(index) = Self::index_of_folder(&folders, &full).map_err(SiblingNavError::InvalidPath)? else {
            return Ok(SiblingImageFolders::default());
        };
        let previous = self.find_image_folder(&folders, index, -1, cancel)?;
        let next = self.find_image_folder(&folders, index, 1, cancel)?;
        Ok(SiblingImageFolders { previous, next })
    }

    /// C# private `IndexOfFolder(folders, currentFolder)`: the first entry equal to `current_folder` ordinally ignoring case,
    /// after `get_full_path` and `trim_ending_directory_separator` on BOTH sides (a caller's trailing separator must not make
    /// the folder "not found"). `Ok(None)` is C#'s -1.
    fn index_of_folder(folders: &[String], current_folder: &str) -> Result<Option<usize>, PathError> {
        let full = get_full_path(current_folder)?;
        let target = trim_ending_directory_separator(&full);
        for (i, folder) in folders.iter().enumerate() {
            let full_folder = get_full_path(folder)?;
            if eq_ignore_case(trim_ending_directory_separator(&full_folder), target) {
                return Ok(Some(i));
            }
        }
        Ok(None)
    }

    /// C# private `FindImageFolder(folders, index, direction, cancellationToken)`. `direction` not +-1: `Err(InvalidDirection)`.
    /// Reads the two getters ONCE per call (absent means `false`). Walks `index + direction`, `index + 2 * direction`, ... while
    /// inside the list; before each one `cancel.check()`; the sibling qualifies when
    /// `file_system.enumerate_files(folder, "*")` lists at least one path with
    /// `pr_core::catalog::types::is_supported_with(path, raw_enabled, webp_heic_enabled)`. ANY `io::Error` of the listing skips that
    /// sibling (C# catches `IOException` and `UnauthorizedAccessException` only; see contract note on O2).
    fn find_image_folder(
        &self,
        folders: &[String],
        index: usize,
        direction: i32,
        cancel: &CancelToken,
    ) -> Result<Option<String>, SiblingNavError> {
        if direction != -1 && direction != 1 {
            return Err(SiblingNavError::InvalidDirection(direction));
        }
        let raw_enabled = self.get_raw_enabled.as_ref().is_some_and(|g| g());
        let webp_heic_enabled = self.get_webp_heic_enabled.as_ref().is_some_and(|g| g());
        let mut i = index as i64 + i64::from(direction);
        while i >= 0 && (i as usize) < folders.len() {
            cancel.check().map_err(|_| SiblingNavError::Cancelled)?;
            let folder = &folders[i as usize];
            // Any listing error skips the sibling (contract O2).
            if let Ok(files) = self.file_system.enumerate_files(Path::new(folder), "*")
                && files
                    .iter()
                    .any(|p| is_supported_with(&p.to_string_lossy(), raw_enabled, webp_heic_enabled))
            {
                return Ok(Some(folder.clone()));
            }
            i += i64::from(direction);
        }
        Ok(None)
    }
}

impl fmt::Debug for SiblingFolderNavigator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SiblingFolderNavigator")
            .field("has_raw_getter", &self.get_raw_enabled.is_some())
            .field("has_webp_heic_getter", &self.get_webp_heic_enabled.is_some())
            .finish_non_exhaustive()
    }
}
