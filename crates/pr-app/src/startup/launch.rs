//! Port of `PhotoReview.Shell.Win32/Startup/ShellLaunch.cs` (49 lines, baseline `d92d6aae`): `ShellInstanceNames` (lines 11-14) and `ShellLaunch`
//! (lines 26-49). Unit W8-17a. Contract: `contracts/W8-17a.md`.
//!
//! The command line of the shell after the shell-only flag (`--smoke`) is removed: folders are made canonical (`misc::launch_args`), the first existing
//! FILE is the image to open, otherwise the first existing FOLDER; the folder of that file (or the folder itself) is the single-instance lock folder.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use pr_core::abstractions::{FileSystem, Log};

/// C# `ShellLaunch.SmokeArgument`.
pub const SMOKE_ARGUMENT: &str = "--smoke";

/// C# `ShellInstanceNames.Prefix` (`"PhotoReviewShell"` in C#). Rust: `pr_core::identity::INSTANCE_PREFIX` (`"PhotoReviewRs"`, R0002): it differs from both
/// C# prefixes, so the Rust app never forwards to a C# instance (D-W8B6-2).
pub const INSTANCE_NAME_PREFIX: &str = pr_core::identity::INSTANCE_PREFIX;

/// C# `ShellLaunch` record. Built by [`ShellLaunch::parse`]; the fields are the record members, `existing_paths` is the C# property of the same name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellLaunch {
    /// Arguments after `--smoke` was removed and folders were normalised (same order and count).
    pub arguments: Vec<String>,
    /// The first argument that is an existing file.
    pub initial_file: Option<String>,
    /// The first argument that is an existing folder.
    pub initial_folder: Option<String>,
    /// The folder of `initial_file` (of its full path), else `initial_folder`.
    pub launch_folder: Option<String>,
    /// `--smoke` was present (exact, case-sensitive).
    pub smoke: bool,
    existing_paths: Vec<String>,
}

impl ShellLaunch {
    /// C# `InitialPath`: the file wins over the folder.
    pub fn initial_path(&self) -> Option<&str> {
        todo!()
    }

    /// C# `ExistingPaths`: the FULL path of every argument that is an existing file or folder (what a second instance forwards).
    pub fn existing_paths(&self) -> &[String] {
        todo!()
    }

    /// C# `ShellLaunch.Parse(args)` with the file system and the log as arguments. `normalize` failures (`PathError`) keep the raw argument and are logged once
    /// (W8-04d open question 3); see the contract for the exact order.
    pub fn parse<S: AsRef<str>>(args: &[S], fs: &dyn FileSystem, log: &dyn Log) -> ShellLaunch {
        todo!()
    }
}
