//! Port of `PhotoReview.App.Shared/Menus/MenuContracts.cs` (C-12, NO-WPF-EXEC-PLAN section 5): the shared menu model
//! and the build context of the right-click menu. Pure data: no window, no `windows` crate. The Win32 host that shows
//! the model is `pr_platform_win::shell_menus` (W8-14e/W8-14f).
//!
//! Unit W8-02a. Contract: `contracts/W8-02a.md`. Complete in the stub (types only, no `todo!()`); the porter may add
//! derives or doc text but must keep names, field order and types, because the golden and the builder depend on them.

use pr_core::settings::app_settings::AppSettings;

/// `MenuItemKind`: the C# enum values, in declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MenuItemKind {
    Command = 0,
    Toggle = 1,
    Submenu = 2,
    Separator = 3,
}

/// `MenuItemModel` (a positional record in C#; the field order is the record's order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItemModel {
    /// The `ContextMenuItemId` name (`"Undo"`, `"ZoomSubmenu"`), a child id (`"Zoom.Preset.150"`, `"Zoom.Custom"`,
    /// `"Zoom.AlsoSetClickLevel"`, `"Zoom.SetCurrentAsClickLevel"`), a separator (`"Separator.<group>"` at the top level,
    /// `"Separator.Zoom.<group>"` in the Zoom submenu) or a Tools command (`"Tools.*"`).
    pub id: String,
    pub kind: MenuItemKind,
    /// Text already localised through `pr_l10n::tr`; empty for a separator.
    pub text: String,
    /// Accelerator text on the right; `None` when the shortcut is absent, empty or whitespace only.
    pub shortcut_text: Option<String>,
    pub is_enabled: bool,
    /// Check mark (toggles only).
    pub is_checked: bool,
    /// Children of a submenu; empty for every other kind.
    pub children: Vec<MenuItemModel>,
    /// Accessibility name; `None` for a separator.
    pub automation_name: Option<String>,
}

/// `ContextMenuBuildContext`: what the menu is built over. The C# record is positional with these names.
///
/// `can_undo`, `is_fit`, `can_open_in_external_editor` and `is_compare_visible` are part of the C# record but do not
/// change the built menu (the builder keeps the WPF behaviour, see `builder`); they are kept so callers pass the same data.
#[derive(Debug, Clone, Copy)]
pub struct ContextMenuBuildContext<'a> {
    pub settings: &'a AppSettings,
    /// A photo is open.
    pub has_image: bool,
    pub can_undo: bool,
    pub is_fit: bool,
    /// The effective zoom factor (1.0 = 100 %), `None` in Fit.
    pub effective_zoom: Option<f64>,
    pub can_open_in_external_editor: bool,
    /// Together with `has_image` this decides the Copy items (`MainViewModel.CanCopyCurrentFilePath`).
    pub can_copy_path: bool,
    pub is_compare_visible: bool,
}
