//! Port of `PhotoReview.App.Shared/Coordinators/FileActionController.cs` lines 723 to 858: `MoveOrCopyToFolderAsync`,
//! `RestoresOutsideFolder`, `ReloadPathAfterUndo`, `IsInFolder` (contract `contracts/W8-07e.md`).
//! Target: `coordinators::file_actions::move_copy`.
use std::path::Path;

use pr_core::catalog::types::is_supported_with;
use pr_core::file_actions::destination_policy::{PickedFolderCheck, validate_picked_folder};
use pr_core::file_actions::types::UndoResult;
use pr_core::model::FileOperationType;
use pr_core::text::dotnet_path::{get_directory_name, get_file_name, get_full_path, trim_ending_directory_separator};
use pr_core::text::eq_ignore_case;
use pr_l10n::keys;
use pr_l10n::template::LocArg;

use super::actions::{AfterCore, CoreTail, ParkedTail};
use super::recycle::{CorePrep, FileActionController, FileActionCx, FileActionDone, FileActionStep, PresentState};
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

fn done(succeeded: bool) -> FileActionStep {
    FileActionStep::Done(FileActionDone::Finished { succeeded })
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
        if !matches!(operation, FileOperationType::Move | FileOperationType::Copy) {
            return Err(operation);
        }
        if cx.catalog.count() == 0 || self.deps.file_actions.as_ref().is_none_or(|f| f.is_busy()) {
            return Ok(done(false));
        }

        let (compare_selected, current) = get_source();
        let Some(source) = compare_selected.clone().or(current.clone()).filter(|s| !s.is_empty()) else {
            return Ok(done(false));
        };
        let Some(photo_folder) = get_directory_name(&source).filter(|f| !f.is_empty()) else {
            return Ok(done(false));
        };

        let is_move = operation == FileOperationType::Move;
        let action_name = self.deps.texts.get(if is_move {
            keys::ACTION_MOVE_TO_FOLDER_NAME
        } else {
            keys::ACTION_COPY_TO_FOLDER_NAME
        });
        let settings = (self.deps.get_settings)();
        let last_folder = if is_move {
            settings.last_move_to_folder.clone()
        } else {
            settings.last_copy_to_folder.clone()
        };
        let file_system = self.deps.file_system.clone();
        let directory_exists = move |path: &str| match &file_system {
            Some(fs) => fs.directory_exists(Path::new(path)),
            None => Path::new(path).is_dir(),
        };
        let last_usable = matches!(
            validate_picked_folder(last_folder.as_deref(), &photo_folder, &directory_exists),
            Ok(PickedFolderCheck::Ok)
        );

        let destination = if settings.move_copy_reuse_last_folder && !force_picker && last_usable {
            last_folder.clone()
        } else {
            let Some(picker) = picker else {
                return Ok(done(false));
            };
            let initial_folder = match last_folder.as_deref() {
                Some(last) if directory_exists(last) => last.to_string(),
                _ => get_directory_name(&photo_folder).unwrap_or_else(|| photo_folder.clone()),
            };

            // The picker runs a nested loop: a forwarded open can switch the folder, and anything that still reaches the view
            // model can change the photo or start a file operation meanwhile (R7-4).
            let folder_before = self.deps.clock.current_folder();
            let title = self.deps.texts.get(if is_move {
                keys::DIALOG_MOVE_TO_FOLDER_TITLE
            } else {
                keys::DIALOG_COPY_TO_FOLDER_TITLE
            });
            let Some(picked) = picker.pick_folder(&title, Some(&initial_folder)) else {
                return Ok(done(false)); // cancelled: no-op
            };

            let (compare_after, current_after) = get_source();
            let after = compare_after.or(current_after);
            if self.deps.clock.current_folder() != folder_before
                || self.deps.file_actions.as_ref().is_some_and(|f| f.is_busy())
                || !after.as_deref().is_some_and(|a| eq_ignore_case(a, &source))
                || cx.catalog.index_of(&source) < 0
            {
                cx.sink.set_status_text(&self.deps.texts.format(
                    keys::STATUS_MOVE_COPY_TO_STATE_CHANGED,
                    &[LocArg::new("actionName", action_name)],
                ));
                return Ok(done(false));
            }
            Some(picked)
        };

        let name_arg = || LocArg::new("actionName", action_name.clone());
        match validate_picked_folder(destination.as_deref(), &photo_folder, &directory_exists) {
            Ok(PickedFolderCheck::Ok) => {}
            Ok(PickedFolderCheck::SameAsPhotoFolder) => {
                cx.sink.set_status_text(
                    &self
                        .deps
                        .texts
                        .format(keys::STATUS_MOVE_COPY_TO_SAME_FOLDER, &[name_arg()]),
                );
                return Ok(done(false));
            }
            Ok(PickedFolderCheck::Missing) => {
                cx.sink.set_status_text(&self.deps.texts.format(
                    keys::STATUS_MOVE_COPY_TO_FOLDER_MISSING,
                    &[
                        name_arg(),
                        LocArg::new("folder", destination.clone().unwrap_or_default()),
                    ],
                ));
                return Ok(done(false));
            }
            // Empty, NotAbsolute and (D-07e-1) a path error: the destination is not a usable full path.
            Ok(PickedFolderCheck::Empty | PickedFolderCheck::NotAbsolute) | Err(_) => {
                cx.sink.set_status_text(
                    &self
                        .deps
                        .texts
                        .format(keys::STATUS_MOVE_COPY_TO_NOT_ABSOLUTE, &[name_arg()]),
                );
                return Ok(done(false));
            }
        }

        let Some(destination) = destination else {
            return Ok(done(false));
        };
        let Ok(full) = get_full_path(&destination) else {
            return Ok(done(false)); // unreachable after a successful validation
        };
        let folder = trim_ending_directory_separator(&full).to_string();
        let tail = MoveCopyTail {
            operation,
            folder: folder.clone(),
            last_folder,
            file_name: get_file_name(&source).to_string(),
        };
        let step = match self.prepare_core(
            cx,
            &action_name,
            operation,
            Some(&folder),
            compare_selected.as_deref(),
            current.as_deref(),
        ) {
            CorePrep::Refused => done(false),
            CorePrep::Ready(prepared) => self.execute_core(cx, prepared, AfterCore::MoveCopyTo(tail)),
        };
        Ok(step)
    }

    /// What follows a core that returned true for Move-to / Copy-to (lines 795 to 813): remember the folder, then write the final
    /// status once the next photo's present finished (RV-A09) and only while the folder is still the one the action ran in.
    pub(super) fn move_copy_after_core(&mut self, cx: &mut FileActionCx<'_>, tail: MoveCopyTail) -> FileActionStep {
        let same_as_last = tail
            .last_folder
            .as_deref()
            .is_some_and(|last| eq_ignore_case(&tail.folder, last));
        if !same_as_last && let Some(remember) = self.deps.remember_folder.as_mut() {
            remember(tail.operation, &tail.folder);
        }
        if self.present == PresentState::Running {
            self.parked = Some(ParkedTail::Core(CoreTail::MoveCopyFinal(tail)));
            return FileActionStep::Pending;
        }
        let check_folder = matches!(self.present, PresentState::Finished { .. });
        self.run_move_copy_final(cx, &tail, check_folder)
    }

    /// The final status of Move-to / Copy-to (lines 805 to 813).
    pub(super) fn run_move_copy_final(
        &mut self,
        cx: &mut FileActionCx<'_>,
        tail: &MoveCopyTail,
        check_folder: bool,
    ) -> FileActionStep {
        if check_folder && !self.deps.clock.is_folder_current(self.last_action_folder_gen) {
            return done(true);
        }
        if cx.catalog.count() > 0 {
            let key = if tail.operation == FileOperationType::Move {
                keys::STATUS_MOVED_TO_FOLDER
            } else {
                keys::STATUS_COPIED_TO_FOLDER
            };
            let message = self.deps.texts.format(
                key,
                &[
                    LocArg::new("fileName", tail.file_name.clone()),
                    LocArg::new("folder", tail.folder.clone()),
                ],
            );
            // Keep the "companion file was missing" warning the core step wrote instead of overwriting it.
            let status = match &self.last_partner_missing_warning {
                Some(warning) => format!("{message} {warning}"),
                None => message,
            };
            cx.sink.set_status_text(&status);
        }
        done(true)
    }

    /// C# `RestoresOutsideFolder(result, currentFolder)` (pure): true when the undo succeeded, has a source, is a Recycle or
    /// Move, and (restored paths exist -> any of them is outside `current_folder`; otherwise the source is outside).
    /// A missing/empty `current_folder` or an invalid path counts as outside.
    pub fn restores_outside_folder(result: Option<&UndoResult>, current_folder: Option<&str>) -> bool {
        let Some(result) = result else {
            return false;
        };
        // C# `result.Operation is Recycle or Move` is the pattern `is (Recycle or Move)`.
        if !result.succeeded
            || result.source.is_empty()
            || !matches!(
                result.operation,
                Some(FileOperationType::Recycle | FileOperationType::Move)
            )
        {
            return false;
        }
        match result.restored_paths.as_ref().filter(|p| !p.is_empty()) {
            Some(paths) => paths.iter().any(|p| !Self::is_in_folder(p, current_folder)),
            None => !Self::is_in_folder(&result.source, current_folder),
        }
    }

    /// C# `ReloadPathAfterUndo(result)` (pure): no restored paths -> the source; the source when it is among the restored paths
    /// (OrdinalIgnoreCase, the source's own spelling is returned); else the first restored path that
    /// `is_supported_with(path, true, true)` (sidecars never qualify), else the source.
    pub fn reload_path_after_undo(result: &UndoResult) -> Option<String> {
        let source = Some(result.source.clone());
        let Some(restored) = result.restored_paths.as_ref().filter(|p| !p.is_empty()) else {
            return source;
        };
        if !result.source.is_empty() && restored.iter().any(|p| eq_ignore_case(p, &result.source)) {
            return source;
        }
        restored
            .iter()
            .find(|p| is_supported_with(p, true, true))
            .cloned()
            .or(source)
    }

    /// C# `IsInFolder(path, folder)` (private static): the parent directory of `path` equals `folder` after `get_full_path` and
    /// `trim_ending_directory_separator`, OrdinalIgnoreCase. Empty folder, no parent, or a path error -> false.
    pub(super) fn is_in_folder(path: &str, folder: Option<&str>) -> bool {
        let Some(folder) = folder.filter(|f| !f.is_empty()) else {
            return false;
        };
        let Some(parent) = get_directory_name(path).filter(|p| !p.is_empty()) else {
            return false;
        };
        match (get_full_path(&parent), get_full_path(folder)) {
            (Ok(parent_full), Ok(folder_full)) => eq_ignore_case(
                trim_ending_directory_separator(&parent_full),
                trim_ending_directory_separator(&folder_full),
            ),
            _ => false,
        }
    }
}
