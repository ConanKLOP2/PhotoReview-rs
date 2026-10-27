//! Port of `PhotoReview.App.Shared/Coordinators/{FileActionController,FileRenameResult}.cs` rename slices at 7eaee2a2.
use super::file_actions::recycle::{FileActionController, FileActionCx, FileActionStep};
use pr_core::file_actions::rename::PathKeyMap;

/// C# `FileRenameStatus`, in declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FileRenameStatus {
    Renamed = 0,
    NoChange = 1,
    Invalid = 2,
    Failed = 3,
    Busy = 4,
    NothingToRename = 5,
}

/// C# `FileRenameResult` with a status and optional target/error details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRenameResult {
    pub status: FileRenameStatus,
    pub new_path: Option<String>,
    pub problem: pr_core::file_actions::rename::RenameNameProblem,
    pub invalid_character: Option<u16>,
    pub error: Option<String>,
}

impl FileActionController {
    /// Start C# `RenameAsync` on the file-action lane; return a pending step when I/O was queued.
    pub fn rename(
        &mut self,
        cx: &mut FileActionCx<'_>,
        compare_selected_path: Option<&str>,
        current_path: Option<&str>,
        typed_name: &str,
    ) -> FileActionStep {
        self.rename_action(cx, compare_selected_path, current_path, typed_name)
    }
    /// Re-key a successful same-folder rename undo without reinserting the catalog entry.
    pub fn handle_rename_undo(&mut self, cx: &mut FileActionCx<'_>, renames: &PathKeyMap) -> FileActionStep {
        self.rename_undo(cx, renames)
    }
}
