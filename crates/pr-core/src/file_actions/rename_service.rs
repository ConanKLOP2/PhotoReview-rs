//! Port of `FileActionService.RenameAsync` / `RenameGroupAsync` at 7eaee2a2 (W9-R2). The shared private core
//! (`ExecuteCoreAsync`, `IsUsableRenameTarget`) stays in `service.rs`.
use super::rename::RenameRequest;
use super::service::FileActionService;
use super::types::{CaptureGroupActionResult, FileActionResult};
use crate::catalog::types::CaptureGroup;

impl FileActionService {
    /// Blocking twin of C# `RenameAsync`; run on the file-action lane (R0003).
    pub fn rename(&self, request: &RenameRequest, cancel: &crate::sync::CancelToken) -> FileActionResult {
        let mut action =
            super::types::FileActionRequest::new(request.source.clone(), crate::model::FileOperationType::Move);
        action.destination = Some(crate::text::dotnet_path::get_directory_name(&request.source).unwrap_or_default());
        self.execute_rename(&action, &request.new_file_name, cancel)
    }
    /// Blocking twin of C# `RenameGroupAsync`; one journal group transaction.
    pub fn rename_group(
        &self,
        group: &CaptureGroup,
        typed_stem: &str,
        cancel: &crate::sync::CancelToken,
    ) -> CaptureGroupActionResult {
        let plan = super::rename::CaptureGroupRenamePlan::build(group, Some(typed_stem), false);
        let Some(names) = plan.file_names.filter(|_| plan.check.is_valid()) else {
            return CaptureGroupActionResult::new(
                false,
                false,
                crate::model::FileOperationType::Move,
                String::new(),
                None,
                Vec::new(),
                Some(self.rename_invalid_name(typed_stem)),
            );
        };
        let mut request =
            super::types::CaptureGroupActionRequest::new(group.clone(), crate::model::FileOperationType::Move);
        request.destination_file_names = Some(names);
        self.execute_group_rename(&request, cancel)
    }
}
