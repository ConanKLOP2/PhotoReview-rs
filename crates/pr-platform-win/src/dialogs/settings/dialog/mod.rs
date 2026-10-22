//! The Settings window (Wave 8 B5, units W8-19j .. W8-19m; port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/SettingsDialog.cs`, 1512 lines, baseline `d92d6aae`).
//! Contracts: `contracts/W8-19j.md` .. `contracts/W8-19m.md`; decisions D-W8B5-901 .. D-W8B5-9xx (`contracts/drafts-b5/G9-decisions.md`).
//!
//! The C# class `SettingsDialog : NativeDialog` is ONE `SettingsContent` (this directory; it implements `DialogContent` and, through the weak adapter
//! `host::PageHost`, `SettingsPageHost`) wrapped by the handle `SettingsDialog` (`frame.rs`: the `NativeDialog` plus the content). The 1512 C# lines are cut at METHOD
//! boundaries, not at the plan's line numbers (D-W8B5-902). One `impl SettingsContent` block per file; the unit that owns a file is the only one that edits it.
//!
//! | file | unit | C# members (baseline lines) |
//! |---|---|---|
//! | `frame` | 19j | `SettingsDialogOptions` (17-52), ids and fields (54-117), constructor and properties (119-196), `BuildItems`, `Flatten` (200-269), `OnInitialized`, `FocusExternalEditor` (271-331); the `DialogContent` impl and every forwarding method of the handle |
//! | `create` | 19j | `EnsurePageCreated`, `EnsureAllPagesCreated`, `IsPageCreated` (333-438) |
//! | `nav` | 19j | `ActivatePage`, `CyclePage`, `HandleKeyForTest`, `OnDialogKey` (566-631), `PageOf` (778) |
//! | `state` | 19k | `WorkingFields`, `ActionsJson`, `RefreshActionsJsonFromPages`, `SetActionsJson`, `LoadAll`, `CollectAll` (464-564) |
//! | `dirty` | 19k | the control accessors for pages and `RelayoutActivePage` (1097-1181, 1418) |
//! | `keybox` | 19k | `IsKeyBox`, `CaptureKey`, `HookKeyBox`, `KeyBoxProc`, `KeyBoxColors`, `IsKeyBoxId` (633-743, 1421) |
//! | `host` | 19k | Rust only: `PageHost`, the `SettingsPageHost` adapter (what `SettingsPageContext` is a facade of) |
//! | `apply` | 19l | `OnCommand`, `OnOk`, `TrySaveAll`, `ResetActivePage`, `ExportSettings`, `ImportSettings`, `ImportSettingsFrom`, `Warn` (745-776, 795-813, 834-907) |
//! | `validate` | 19l | `FocusField` (815-832) and the pure plan of a save result |
//! | `messages` | 19l | `Forward`, `ForwardProc`, `OnWindowMessage`, `PostToUi`, `OnLocalizerChanged`, `RefreshLocalizedTexts`, `ApplyTextsToControls` (909-1095) |
//! | `footer` | 19m | `Margin` .. `RowWidth`, the button rows, `PreferredClientSize`, `MinimumClientSize`, `LayoutControls`, `PlaceRow` (1183-1279) |
//! | `page_layout` | 19m | `OnScroll`, `LayoutPage`, `UpdateScrollBar`, `SetScroll`, `ScrollByBar`, `ScrollByWheel`, `EnsureVisible` (780-793, 1281-1410) |
//! | `surface` | 19m | `StaticTextColor`, `PageSurface` (1412-1416, 1424-1512) |
//! | `close` | 19m | `OnClosed` (440-462) |
//!
//! Lead-owned index (this file is final: it declares the submodules and re-exports the public surface only).

pub mod apply;
pub mod close;
pub mod create;
pub mod dirty;
pub mod footer;
pub mod frame;
pub mod host;
pub mod keybox;
pub mod messages;
pub mod nav;
pub mod page_layout;
pub mod state;
pub mod surface;
pub mod validate;

pub use self::frame::{
    OpenUrlFn, PageHandle, SettingsContent, SettingsDialog, SettingsDialogArgs, SettingsDialogError,
    SettingsDialogOptions, ShowActionProfilesFn, StartExplorerFn,
};
