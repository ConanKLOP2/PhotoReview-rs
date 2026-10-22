//! Port of `PhotoReview.Shell.Win32/Dialogs/Native/NativeDialog.cs` (1459 lines, baseline `d92d6aae`), the base class of every native dialog.
//! Units W8-18h, W8-18i, W8-18j, W8-18k. Contracts: `contracts/W8-18h.md` .. `contracts/W8-18k.md`; decisions D-W8B5-301 .. D-W8B5-318.
//!
//! The C# class is `abstract` with `virtual` hooks; Rust has the trait [`DialogContent`] for the hooks and the handle [`NativeDialog`] (an `Rc` of the shared
//! state) for everything else (D-W8B5-301). One `impl NativeDialog` block per file; the C# `partial`-like slices follow METHOD boundaries, not the plan's
//! line numbers (D-W8B5-302):
//!
//! | file | unit | C# members |
//! |---|---|---|
//! | `core` | 18h | fields, constructor, properties, the virtual hooks, `DialogProc`, `RethrowPending`, `HandleMessage` |
//! | `modal` | 18h | `ShowModal`, `ShowModeless`, `Activate`, `Close`, `EndModal`, `ModelessFilter`, `RunLoop`, `ClaimDialogMessage`, `DialogStyle`, `DialogExStyle` |
//! | `dpi` | 18i | `DpiFor`, `ApplyFonts`, `CreateFonts`, `MeasureUnits`, `Dx`, `Dy`, `LineHeight`, `OnDpiChanged` |
//! | `layout` | 18i | child registry, `Item`, `Place`, text and measure helpers, button rows, `WindowSizeForClient`, `ResizeClient`, `RunLayout`, `PlaceInitially`, `WM_SIZE`, `WM_GETMINMAXINFO` |
//! | `focus` | 18j | `InitializeDialog`, `FirstTabStop`, `CenterSingleLineEdits` |
//! | `events` | 18j | `HandleCommand`, `HandleNotify`, scroll, list view header hook, `SetListViewSort`, `IsEdit`, `IsButton` |
//! | `tooltip` | 18j | `SetTooltip`, `SetItemTooltips`, `EnsureTooltip`, `AddTool`, `ProvideTipText`, `EnsureTipBuffer`, the item tip subclass |
//! | `draw` | 18k | `DrawOwnerItem`, the `WM_CTLCOLOR*` answers (brushes), `WM_SETCURSOR` |
//! | `result` | 18k | `Cleanup`, `WM_CLOSE`, `DeleteObject` |
//!
//! Lead-owned index (this file is final: it declares the submodules and re-exports the public surface only). Do NOT write `core::` paths in this file: `core` is
//! also the name of the first submodule (use `std::`).

pub mod core;
pub mod dpi;
pub mod draw;
pub mod events;
pub mod focus;
pub mod layout;
pub mod modal;
pub mod result;
pub mod tooltip;

pub use self::core::{
    DialogContent, DialogError, ID_CANCEL, ID_OK, ListItemText, NativeDialog, WS_DEFAULT_DIALOG, default_chrome,
};
pub use self::tooltip::ItemTipProvider;
