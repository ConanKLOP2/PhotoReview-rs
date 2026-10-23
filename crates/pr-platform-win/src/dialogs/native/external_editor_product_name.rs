//! W9-E3: port of `PhotoReview.Shell.Win32/Dialogs/Native/ExternalEditorProductName.cs` (34 lines, baseline `7eaee2a2`): the suggested menu name of an editor program =
//! the `ProductName`, else the `FileDescription`, of the executable's version resource. Contract: `contracts/W9-E3.md`. Every body is `todo!()`.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

/// C# `ExternalEditorProductName.Suggest(exePath)`: `None` for a blank path, an unreadable file or when both texts are blank (the caller then uses
/// `ExternalEditors::name_from_path`). Reads `ProductName` then `FileDescription` (`GetFileVersionInfoSizeW` / `GetFileVersionInfoW` / `VerQueryValueW`, the
/// `Win32_Storage_FileSystem` feature is already enabled; the translation is the first of `\VarFileInfo\Translation`, as `FileVersionInfo` does) and returns the
/// first non-blank one trimmed. Never panics on a bad file; all `unsafe` is here with a SAFETY comment.
pub fn suggest(exe_path: &str) -> Option<String> {
    todo!()
}

/// The pure choice of `suggest`, testable without a file: the first of `product_name`, `file_description` that is not blank (`trim().is_empty()`), trimmed.
pub fn first_non_blank(product_name: Option<&str>, file_description: Option<&str>) -> Option<String> {
    todo!()
}
