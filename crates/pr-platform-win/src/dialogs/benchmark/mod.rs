//! Port of `PhotoReview.Shell.Win32/Dialogs/Native/BenchmarkDialog.cs` (661 lines, baseline `d92d6aae`). Units W8-18r and W8-18s. Contracts: `contracts/W8-18r.md`,
//! `contracts/W8-18s.md`; decisions D-W8B5-501 .. D-W8B5-520.
//!
//! The Win32 view of `BenchmarkPresenter` (WPF `BenchmarkWindow`): a MODELESS, resizable window with the folder and image-limit boxes, the profile list, Run / Quick check
//! / Cancel, a progress bar and status line, the sortable result grid and Open report / Open result folder / Close. The presenter owns every rule; the view copies values
//! into the controls. Tab, Esc and Enter reach `IsDialogMessage` through the shell loop's modeless filter.
//!
//! The plan cuts the C# file at line 330 into "layout, state" (18r) and "run, results" (18s). The real cut follows the C# members (D-W8B5-502): 18r = constants, items,
//! constructor, `OnInitialized`, columns, `OnClosed`, language change, the grid rebuild and sort, and ALL of the layout (`layout.rs`, `state.rs`); 18s = the notifications
//! and row colours of the grid, the profile list text, the commands, the run driver and the `IBenchmarkView` implementation (`results.rs`, `run.rs`). `state.rs` holds the
//! one `impl DialogContent`; its hooks delegate to inherent methods that `results.rs` and `run.rs` add to `BenchmarkContent`.
//!
//! Lead-owned index (this file is final: it declares the submodules and re-exports the public surface only).

pub mod layout;
pub mod results;
pub mod run;
pub mod state;

pub use self::state::{BenchmarkContent, BenchmarkDialog, BenchmarkOptions, OpenInExplorerFn, PickFolderFn};
