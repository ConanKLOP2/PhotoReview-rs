//! Port of `PhotoReview.Shell.Win32/Startup/ShellLaunch.cs` (49 lines, baseline `d92d6aae`): `ShellInstanceNames` (lines 11-14) and `ShellLaunch`
//! (lines 26-49). Unit W8-17a. Contract: `contracts/W8-17a.md`.
//!
//! The command line of the shell after the shell-only flag (`--smoke`) is removed: folders are made canonical (`misc::launch_args`), the first existing
//! FILE is the image to open, otherwise the first existing FOLDER; the folder of that file (or the folder itself) is the single-instance lock folder.
use pr_core::abstractions::{FileSystem, Log};
use pr_core::text::dotnet_path::{get_directory_name, get_full_path};

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
        self.initial_file.as_deref().or(self.initial_folder.as_deref())
    }

    /// C# `ExistingPaths`: the FULL path of every argument that is an existing file or folder (what a second instance forwards).
    pub fn existing_paths(&self) -> &[String] {
        &self.existing_paths
    }

    /// C# `ShellLaunch.Parse(args)` with the file system and the log as arguments. `normalize` failures (`PathError`) keep the raw argument and are logged once
    /// (W8-04d open question 3); see the contract for the exact order.
    pub fn parse<S: AsRef<str>>(args: &[S], fs: &dyn FileSystem, log: &dyn Log) -> ShellLaunch {
        let smoke = args.iter().any(|arg| arg.as_ref() == SMOKE_ARGUMENT);
        let mut arguments = Vec::with_capacity(args.len());
        let mut failed = 0usize;
        for arg in args.iter().map(AsRef::as_ref).filter(|arg| *arg != SMOKE_ARGUMENT) {
            match crate::misc::launch_args::normalize_one(fs, arg) {
                Ok(normalized) => arguments.push(normalized),
                Err(_) => {
                    failed += 1;
                    arguments.push(arg.to_owned());
                }
            }
        }
        if failed != 0 {
            log.warn(&format!(
                "Launch arguments could not be normalised and were kept as given: {failed}"
            ));
        }

        let initial_file = arguments
            .iter()
            .find(|arg| fs.file_exists(std::path::Path::new(arg)))
            .cloned();
        let initial_folder = arguments
            .iter()
            .find(|arg| fs.directory_exists(std::path::Path::new(arg)))
            .cloned();
        let launch_folder = initial_file
            .as_deref()
            .and_then(|file| {
                let full = get_full_path(file).unwrap_or_else(|_| file.to_owned());
                get_directory_name(&full)
            })
            .or_else(|| initial_folder.clone());
        let existing_paths = arguments
            .iter()
            .filter(|arg| {
                let path = std::path::Path::new(arg.as_str());
                fs.file_exists(path) || fs.directory_exists(path)
            })
            .map(|arg| get_full_path(arg).unwrap_or_else(|_| arg.clone()))
            .collect();

        ShellLaunch {
            arguments,
            initial_file,
            initial_folder,
            launch_folder,
            smoke,
            existing_paths,
        }
    }
}
