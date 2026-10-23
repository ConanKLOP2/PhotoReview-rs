//! Port of `PhotoReview.Shell.Win32/Dialogs/Native/RenamePromptDialog.cs` at 7eaee2a2.
#![allow(unused_variables, dead_code)] // Stub-only: remove with the last todo.

/// The modal Win32 rename dialog; native window creation and message handling live here.
#[derive(Debug)]
pub struct RenamePromptDialog {
    /// The display text and validator supplied by the view model.
    pub request: pr_core::abstractions::RenamePromptRequest,
}

impl RenamePromptDialog {
    /// C# constructor, with the existing native-dialog chrome supplied by the host later.
    pub fn new(request: pr_core::abstractions::RenamePromptRequest) -> Self {
        todo!()
    }
    /// C# `LabelId`.
    pub const LABEL_ID: i32 = 100;
    /// C# `EditId`.
    pub const EDIT_ID: i32 = 101;
    /// C# `ExtensionId`.
    pub const EXTENSION_ID: i32 = 102;
    /// C# `ErrorId`.
    pub const ERROR_ID: i32 = 103;
    /// C# `CancelId`.
    pub const CANCEL_ID: i32 = 2;
    /// C# `RenameId`.
    pub const RENAME_ID: i32 = 1;
    /// Show the dialog modally and return only the accepted name part.
    pub fn show(&mut self) -> Option<String> {
        todo!()
    }
}
