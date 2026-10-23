//! Port of `PhotoReview.Core/FileActions/{RenameNamePolicy,CaptureGroupRenamePlan,RenameRequest}.cs` at 7eaee2a2 (W9-R1).
//! `CaseOnlyRename` and `FileActionService::rename*` are W9-R2 (`case_only_rename.rs`, `rename_service.rs`).
#![allow(unused_variables, dead_code)] // Stub-only: Wave 9 porters remove this with the last todo.

use crate::catalog::types::CaptureGroup;
use std::collections::HashMap;

/// The C# `RenameNameProblem` discriminants, in declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RenameNameProblem {
    None,
    Empty,
    InvalidCharacter,
    TrailingDotOrSpace,
    ReservedDeviceName,
    NameTooLong,
    PathTooLong,
    SidecarNotMappable,
}

/// Pure name verdict; `new_file_name` includes the original extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameNameCheck {
    pub problem: RenameNameProblem,
    pub new_file_name: String,
    /// C# `char` is one UTF-16 code unit, including an unpaired surrogate.
    pub invalid_character: Option<u16>,
    pub is_unchanged: bool,
    pub differs_only_by_case: bool,
}

impl RenameNameCheck {
    /// C# `IsValid`: `Problem == None`.
    pub fn is_valid(&self) -> bool {
        self.problem == RenameNameProblem::None
    }
}

/// C# `RenameNamePolicy`, with no file-system access.
#[derive(Debug)]
pub struct RenameNamePolicy;
impl RenameNamePolicy {
    pub const MAX_FILE_NAME_LENGTH: usize = 255;
    pub const MAX_PATH: usize = 260;
    /// C# `Check`: `typed_stem` is the user's name part.
    pub fn check(typed_stem: Option<&str>, current_full_path: &str, long_path_aware: bool) -> RenameNameCheck {
        todo!()
    }
    /// Native edit-control boundary that can preserve unpaired UTF-16 units.
    pub fn check_utf16(typed_stem: Option<&[u16]>, current_full_path: &str, long_path_aware: bool) -> RenameNameCheck {
        todo!()
    }
    /// C# `CheckFileName`: structural check of a complete target component.
    pub fn check_file_name(file_name: Option<&str>) -> RenameNameProblem {
        todo!()
    }
    /// C# `ExceedsPathBudget`.
    pub fn exceeds_path_budget(current_full_path: &str, new_file_name: &str, long_path_aware: bool) -> bool {
        todo!()
    }
    /// C# `IsReservedDeviceName`.
    pub fn is_reserved_device_name(file_name: &str) -> bool {
        todo!()
    }
}

/// C# `CaptureGroupRenamePlan.Plan`: path keys use OrdinalIgnoreCase semantics when implemented.
#[derive(Debug, Clone)]
pub struct CaptureGroupRenamePlan {
    pub check: RenameNameCheck,
    pub file_names: Option<HashMap<String, String>>,
}
impl CaptureGroupRenamePlan {
    /// C# `Build` for JPEG, RAW and optional XMP.
    pub fn build(group: &CaptureGroup, typed_stem: Option<&str>, long_path_aware: bool) -> Self {
        todo!()
    }
}

/// C# `RenameRequest`: complete new file name, never a destination folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameRequest {
    pub source: String,
    pub new_file_name: String,
}
