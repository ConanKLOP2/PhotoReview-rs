//! Port of `PhotoReview.Shell.Win32/Dialogs/Native/NativeControls.cs` (552 lines, baseline `d92d6aae`), split at class boundaries (D-W8B5-209):
//! - [`basic`] (W8-18f, C# lines 1-252): `CommonControlsInit`, `ComboBoxControl`, `ListBoxControl` (with the owner-draw row `draw_item`);
//! - [`list_tree`] (W8-18g, C# lines 254-498): `ListViewControl` (report mode, custom draw, header draw);
//! - [`custom`] (W8-18g, C# lines 500-552): `ProgressControl`, `TrackbarControl`, `ButtonStateControl`.
//!
//! WP-35 GD2: each type wraps the messages of ONE standard window class. The dialog manager still handles Tab, accelerators, IME and UI Automation; the dark
//! colours come from `chrome::DarkDialogChrome` (uxtheme off + hand painter) and from the `draw_item` / `custom_draw` helpers here.
//!
//! Lead-owned index (this file is final: it declares the three submodules only).

pub mod basic;
pub mod custom;
pub mod list_tree;
