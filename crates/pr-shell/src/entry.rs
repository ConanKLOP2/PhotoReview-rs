//! Port of `PhotoReview.Shell.Win32/Program.cs` (16 lines, baseline `d92d6aae`): `Main(string[] args) => ShellStartup.Run(args)` and the `SmokeArgument` constant. Unit W8-20a.
//! Contract: `contracts/W8-20a.md`.
//!
//! The binary's `main` calls [`main_entry`]. The logic is split so that everything but the last line is testable without starting a window: [`args_from_os`] (argument decoding),
//! [`run_main`] (decode, run, return the code; the runner is a parameter), [`production_run`] (`ShellStartup::run` with the default options), [`main_entry`] (apartment, run, exit).
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::ffi::OsString;

/// C# `Program.SmokeArgument` (`internal const string`, an alias of `ShellLaunch.SmokeArgument`): `--smoke`.
pub use pr_app::startup::launch::SMOKE_ARGUMENT;

/// The command line the way `Main(string[] args)` sees it: everything AFTER the program name, each argument converted with `OsString::to_string_lossy` (an unpaired surrogate becomes U+FFFD,
/// like the CLR's `string`), order kept, empty arguments kept. `args_from_os(std::env::args_os())` is the production call. An iterator that is empty (no program name) gives an empty vector.
pub fn args_from_os<I: IntoIterator<Item = OsString>>(os_args: I) -> Vec<String> {
    todo!()
}

/// `args_from_os(os_args)`, then `run(&args)`; returns the code of `run` unchanged (no clamping, no mapping: `ShellStartup::run` already returns 0, 1 = `EXIT_STARTUP_FAILED`, 7 =
/// `EXIT_SMOKE_TIMEOUT` or the forced code). Never catches a panic of `run` (`run` never panics by its own contract; a bug must stay visible as exit code 101 after the panic hook logged it).
pub fn run_main<I, F>(os_args: I, run: F) -> i32
where
    I: IntoIterator<Item = OsString>,
    F: FnOnce(&[String]) -> i32,
{
    todo!()
}

/// The production runner: `ShellStartup::run(args, ShellStartupOptions::default())` (the default options read `PHOTOREVIEW_RS_INSTANCE_PREFIX`, `PHOTOREVIEW_RS_WARP`,
/// `PHOTOREVIEW_RS_NOWARM`, D-W8B6-26).
pub fn production_run(args: &[String]) -> i32 {
    todo!()
}

/// What `main` does, in this order, and never returns:
///
/// 1. `StaApartment::enter()` (C# `[STAThread]`). An `Err` is logged `AppLog::global().error_forced("COM apartment", ..)` and the start continues without the guard (drag and drop will not
///    register; everything else works).
/// 2. `code = run_main(std::env::args_os(), production_run)`.
/// 3. The guard is dropped (`CoUninitialize`) AFTER `run` returned, i.e. after the window, the dispatcher and the services are gone.
/// 4. `std::process::exit(code)`: the code is an `i32` (a forced smoke code may exceed `u8`, so `ExitCode` is not used).
pub fn main_entry() -> ! {
    todo!()
}
