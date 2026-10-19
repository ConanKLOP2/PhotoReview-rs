//! Port of `PhotoReview.App.Shared/Coordinators/FileActionController.cs` lines 723 to 858: `MoveOrCopyToFolderAsync`,
//! `RestoresOutsideFolder`, `ReloadPathAfterUndo`, `IsInFolder` (contract `contracts/W8-07e.md`).
//! Target: `coordinators::file_actions::move_copy`.
// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]

use pr_core::file_actions::types::UndoResult;
use pr_core::model::FileOperationType;

use super::recycle::{FileActionController, FileActionCx, FileActionStep};
use crate::coordinators::traits::IFolderPicker;

/// What `MoveOrCopyToFolderAsync` needs after the core finished (lines 795 to 813): C# locals `operation`, `folder`,
/// `lastFolder`, `source`.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveCopyTail {
    pub operation: FileOperationType,
    /// The destination folder: `trim_ending_directory_separator(get_full_path(destination))`.
    pub folder: String,
    /// C# `lastFolder` (the setting value read before the picker).
    pub last_folder: Option<String>,
    /// File name of the photo (`get_file_name(source)`).
    pub file_name: String,
}

impl FileActionController {
    /// C# `MoveOrCopyToFolderAsync(operation, forcePicker, getSource)`.
    /// `operation` other than Move/Copy: returns `Err(operation)` (C# `ArgumentOutOfRangeException`). `get_source` is read live
    /// and again after the picker returns. Early `Done(Finished{false})` returns: empty catalog, no executor, busy executor, empty
    /// source, source without parent folder, no picker when one is needed, picker cancelled, `validate_picked_folder` failed
    /// (each with its status text `StatusMoveCopyTo*`), state changed while the picker was open (status
    /// `StatusMoveCopyToStateChanged`). Else the shared core runs with `AfterCore::MoveCopyTo`; when it succeeded,
    /// `remember_folder(operation, folder)` runs if `folder` differs (OrdinalIgnoreCase) from `last_folder`, then, once the next
    /// photo's present finished and the folder is still the one the action ran in, status `StatusMovedToFolder` /
    /// `StatusCopiedToFolder` (+ the partner-missing warning) when the catalog is not empty.
    pub fn move_or_copy_to_folder(
        &mut self,
        cx: &mut FileActionCx<'_>,
        picker: Option<&mut dyn IFolderPicker>,
        operation: FileOperationType,
        force_picker: bool,
        get_source: &dyn Fn() -> (Option<String>, Option<String>),
    ) -> Result<FileActionStep, FileOperationType> {
        todo!()
    }

    /// C# `RestoresOutsideFolder(result, currentFolder)` (pure): true when the undo succeeded, has a source, is a Recycle or
    /// Move, and (restored paths exist -> any of them is outside `current_folder`; otherwise the source is outside).
    /// A missing/empty `current_folder` or an invalid path counts as outside.
    pub fn restores_outside_folder(result: Option<&UndoResult>, current_folder: Option<&str>) -> bool {
        todo!()
    }

    /// C# `ReloadPathAfterUndo(result)` (pure): no restored paths -> the source; the source when it is among the restored paths
    /// (OrdinalIgnoreCase, the source's own spelling is returned); else the first restored path that
    /// `is_supported_with(path, true, true)`, else the source.
    pub fn reload_path_after_undo(result: &UndoResult) -> Option<String> {
        todo!()
    }

    /// C# `IsInFolder(path, folder)` (private static): the parent directory of `path` equals `folder` after `get_full_path` and
    /// `trim_ending_directory_separator`, OrdinalIgnoreCase. Empty folder, no parent, or a path error -> false.
    pub(super) fn is_in_folder(path: &str, folder: Option<&str>) -> bool {
        todo!()
    }
}
