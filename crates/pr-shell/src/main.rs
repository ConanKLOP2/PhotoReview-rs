//! Entry point of `PhotoReviewRs.exe`: the Rust `Program.cs` (`PhotoReview.Shell.Win32/Program.cs`, 16 lines, baseline `d92d6aae`). Unit W8-20a. Contract: `contracts/W8-20a.md`.
//!
//! The whole start-up sequence is `pr_shell::startup::run::ShellStartup::run`; this file only picks the Windows GUI subsystem (C# `OutputType` `WinExe`: no console window, `--smoke`
//! prints nothing) and calls `pr_shell::entry::main_entry`, which never returns (it ends with `std::process::exit(code)`).
//!
//! The binary lives in `pr-shell`, not in `pr-app` as the plan row W8-20a says: `pr-app` cannot depend on `pr-platform-win` or `pr-shell` (D-W8B1-1, layering PR #91); decision D-W8B6-34.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    pr_shell::entry::main_entry()
}

/// PhotoReviewRs is a Windows application (Win32 + Direct2D, P-06); on another OS the binary only says so, so `cargo check` of the workspace still builds there.
#[cfg(not(windows))]
fn main() {
    eprintln!("PhotoReviewRs runs on Windows only.");
    std::process::exit(2);
}
