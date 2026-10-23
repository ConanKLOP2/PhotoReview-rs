//! Port of `FileActionService.RenameAsync` / `RenameGroupAsync` at 7eaee2a2 (W9-R2). The shared private core
//! (`ExecuteCoreAsync`, `IsUsableRenameTarget`) stays in `service.rs`.
#![allow(unused_variables, dead_code)] // Stub-only: Wave 9 porters remove this with the last todo.

use super::rename::RenameRequest;
use super::service::FileActionService;
use super::types::{CaptureGroupActionResult, FileActionResult};
use crate::catalog::types::CaptureGroup;

impl FileActionService {
    /// Blocking twin of C# `RenameAsync`; run on the file-action lane (R0003).
    pub fn rename(&self, request: &RenameRequest, cancel: &crate::sync::CancelToken) -> FileActionResult {
        todo!()
    }
    /// Blocking twin of C# `RenameGroupAsync`; one journal group transaction.
    pub fn rename_group(
        &self,
        group: &CaptureGroup,
        typed_stem: &str,
        cancel: &crate::sync::CancelToken,
    ) -> CaptureGroupActionResult {
        todo!()
    }
}
