//! The right-click menu customisation: item ids, the item table, and the pure visibility rule.
//!
//! Port of `PhotoReview.Core/Settings/ContextMenuLayout.cs` (new in baseline d92d6aae: `ContextMenuItemId`,
//! `ContextMenuItemInfo`, `ContextMenuContext`, `ContextMenuEntry`, `ContextMenuLayout`, static `ContextMenuItems`).
//! Contract: `contracts/W2-20/contract.md`.
//!
//! The enum member NAMES are written to `config.json` (`AppSettings.HiddenContextMenuItems`), so they and their
//! numeric values (declaration order, 0..=17) never change; new members go at the end.

use std::collections::HashSet;

use crate::settings::app_settings::AppSettings;

/// `ContextMenuItemId`: stable identifiers of the right-click menu items the user can turn on or off.
/// The discriminants are the C# enum values (`Enum.TryParse` accepts them as numbers, see [`parse_hidden`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(i32)]
pub enum ContextMenuItemId {
    Undo = 0,
    MoveToRecycleBin = 1,
    Fit = 2,
    ZoomToLevel = 3,
    ZoomSubmenu = 4,
    OpenFolder = 5,
    NextFolder = 6,
    PreviousFolder = 7,
    ExternalEditor = 8,
    CopyFileName = 9,
    CopyFullPath = 10,
    /// Always shown: switching it off would lock the user out of Settings.
    Settings = 11,
    // Children of the Zoom submenu.
    ZoomFitWidth = 12,
    ZoomFitWidth2 = 13,
    ZoomFitHeight = 14,
    ZoomPresets = 15,
    ZoomLevelOptions = 16,
    /// Refresh (re-render the current view at full quality); sits at the end of the zoom cluster so it follows that
    /// cluster's separator. Declared last (value 17) but placed in group 2 of the table.
    Refresh = 17,
}

impl ContextMenuItemId {
    /// Every member in declaration order (`Enum.GetValues<ContextMenuItemId>()`).
    pub const ALL: [ContextMenuItemId; 18] = [
        Self::Undo,
        Self::MoveToRecycleBin,
        Self::Fit,
        Self::ZoomToLevel,
        Self::ZoomSubmenu,
        Self::OpenFolder,
        Self::NextFolder,
        Self::PreviousFolder,
        Self::ExternalEditor,
        Self::CopyFileName,
        Self::CopyFullPath,
        Self::Settings,
        Self::ZoomFitWidth,
        Self::ZoomFitWidth2,
        Self::ZoomFitHeight,
        Self::ZoomPresets,
        Self::ZoomLevelOptions,
        Self::Refresh,
    ];

    /// The C# identifier (`id.ToString()`), the text stored in `config.json`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Undo => "Undo",
            Self::MoveToRecycleBin => "MoveToRecycleBin",
            Self::Fit => "Fit",
            Self::ZoomToLevel => "ZoomToLevel",
            Self::ZoomSubmenu => "ZoomSubmenu",
            Self::OpenFolder => "OpenFolder",
            Self::NextFolder => "NextFolder",
            Self::PreviousFolder => "PreviousFolder",
            Self::ExternalEditor => "ExternalEditor",
            Self::CopyFileName => "CopyFileName",
            Self::CopyFullPath => "CopyFullPath",
            Self::Settings => "Settings",
            Self::ZoomFitWidth => "ZoomFitWidth",
            Self::ZoomFitWidth2 => "ZoomFitWidth2",
            Self::ZoomFitHeight => "ZoomFitHeight",
            Self::ZoomPresets => "ZoomPresets",
            Self::ZoomLevelOptions => "ZoomLevelOptions",
            Self::Refresh => "Refresh",
        }
    }

    /// The member with this numeric value, if any (`Enum.IsDefined`).
    pub fn from_value(value: i32) -> Option<Self> {
        Self::ALL.iter().copied().find(|id| *id as i32 == value)
    }
}

/// One entry of the item table: where it sits (parent, group), and whether it can be turned off / needs an open photo.
/// `ContextMenuItemInfo(Id, Parent, Group, Locked = false, NeedsPhoto = false)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextMenuItemInfo {
    pub id: ContextMenuItemId,
    pub parent: Option<ContextMenuItemId>,
    pub group: i32,
    pub locked: bool,
    pub needs_photo: bool,
}

/// What the menu is opened over (`ContextMenuContext(HasPhoto)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextMenuContext {
    pub has_photo: bool,
}

/// One line of a computed menu: an item, or (`item` is `None`) the separator drawn right before `group`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextMenuEntry {
    pub item: Option<ContextMenuItemId>,
    pub group: i32,
}

/// The computed menu level: ordered entries (items and separators) to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextMenuLayout {
    /// Ordered lines to show; separators only sit BETWEEN two non-empty groups (never first, last or doubled).
    pub entries: Vec<ContextMenuEntry>,
}

impl ContextMenuLayout {
    /// `IsVisible(id)`: some entry shows this item.
    pub fn is_visible(&self, id: ContextMenuItemId) -> bool {
        self.entries.iter().any(|e| e.item == Some(id))
    }

    /// `HasSeparatorBefore(group)`: some entry is a separator for this group.
    pub fn has_separator_before(&self, group: i32) -> bool {
        self.entries.iter().any(|e| e.item.is_none() && e.group == group)
    }
}

const fn info(
    id: ContextMenuItemId,
    parent: Option<ContextMenuItemId>,
    group: i32,
    locked: bool,
    needs_photo: bool,
) -> ContextMenuItemInfo {
    ContextMenuItemInfo {
        id,
        parent,
        group,
        locked,
        needs_photo,
    }
}

use ContextMenuItemId as Id;

/// `ContextMenuItems.Items`: every item, in menu order (top level first, then the Zoom submenu's children).
/// Order matters: [`compute`] groups by first appearance in this table, and [`to_stored`] emits in this order
/// (so `Refresh` precedes `OpenFolder` even though its enum value is 17).
pub const ITEMS: [ContextMenuItemInfo; 18] = [
    info(Id::Undo, None, 1, false, false),
    info(Id::MoveToRecycleBin, None, 1, false, false),
    info(Id::Fit, None, 2, false, false),
    info(Id::ZoomToLevel, None, 2, false, false),
    info(Id::ZoomSubmenu, None, 2, false, false),
    info(Id::Refresh, None, 2, false, false),
    info(Id::OpenFolder, None, 3, false, false),
    info(Id::NextFolder, None, 3, false, false),
    info(Id::PreviousFolder, None, 3, false, false),
    info(Id::ExternalEditor, None, 4, false, false),
    info(Id::CopyFileName, None, 5, false, true),
    info(Id::CopyFullPath, None, 5, false, true),
    info(Id::Settings, None, 6, true, false),
    info(Id::ZoomFitWidth, Some(Id::ZoomSubmenu), 1, false, false),
    info(Id::ZoomFitWidth2, Some(Id::ZoomSubmenu), 1, false, false),
    info(Id::ZoomFitHeight, Some(Id::ZoomSubmenu), 1, false, false),
    info(Id::ZoomPresets, Some(Id::ZoomSubmenu), 2, false, false),
    info(Id::ZoomLevelOptions, Some(Id::ZoomSubmenu), 3, false, false),
];

/// `ContextMenuItems.DefaultHidden`: the ids hidden by default (the pre-customisation defaults: the zoom cluster and
/// Move to Recycle Bin).
pub const DEFAULT_HIDDEN: [ContextMenuItemId; 4] = [Id::MoveToRecycleBin, Id::Fit, Id::ZoomToLevel, Id::ZoomSubmenu];

/// `ContextMenuItems.Info(id)`: the table row of `id` (C# `Items.First`).
pub fn item_info(id: ContextMenuItemId) -> &'static ContextMenuItemInfo {
    ITEMS.iter().find(|row| row.id == id).unwrap_or(&ITEMS[0]) // every id is in the table (checked by a test); C# would throw here
}

/// `ContextMenuItems.LegacyHidden(settings)`: the set hidden by the three legacy flags. A flag that is false hides
/// every item it used to gate: `show_zoom_menu_items` false hides `Fit`, `ZoomToLevel`, `ZoomSubmenu`;
/// `show_folder_menu_items` false hides `OpenFolder`, `NextFolder`, `PreviousFolder`; `show_recycle_menu_item` false
/// hides `MoveToRecycleBin`.
pub fn legacy_hidden(settings: &AppSettings) -> HashSet<ContextMenuItemId> {
    let mut hidden = HashSet::new();
    if !settings.show_zoom_menu_items {
        hidden.extend([Id::Fit, Id::ZoomToLevel, Id::ZoomSubmenu]);
    }
    if !settings.show_folder_menu_items {
        hidden.extend([Id::OpenFolder, Id::NextFolder, Id::PreviousFolder]);
    }
    if !settings.show_recycle_menu_item {
        hidden.insert(Id::MoveToRecycleBin);
    }
    hidden
}

/// `ContextMenuItems.EffectiveHidden(settings)`: the stored list when there is one (an empty list = show everything),
/// otherwise [`legacy_hidden`]. `Settings` is never in the result.
pub fn effective_hidden(settings: &AppSettings) -> HashSet<ContextMenuItemId> {
    match &settings.hidden_context_menu_items {
        None => legacy_hidden(settings),
        Some(list) => parse_hidden(list),
    }
}

/// `ContextMenuItems.ParseHidden(names)`: the known, unlocked ids named by the stored strings. A `None` element,
/// an unknown or differently cased name and the locked `Settings` item are dropped.
///
/// **.NET quirk:** C# uses `Enum.TryParse<ContextMenuItemId>(name, ignoreCase: false)` and then `Enum.IsDefined`, so
/// besides exact names it accepts numbers and `,`-joined names whose values are OR-ed. The contract gives the exact
/// algorithm; the golden `context-menu` (`single_name` rows) pins it.
pub fn parse_hidden(names: &[Option<String>]) -> HashSet<ContextMenuItemId> {
    names
        .iter()
        .flatten()
        .filter_map(|name| try_parse_enum(name))
        .filter(|id| !item_info(*id).locked)
        .collect()
}

/// `ContextMenuItems.ToStored(hidden)`: the canonical stored form: names of the unlocked ids in `hidden`, in
/// [`ITEMS`] order, no duplicates (`Settings` is dropped).
pub fn to_stored(hidden: impl IntoIterator<Item = ContextMenuItemId>) -> Vec<String> {
    let set: HashSet<ContextMenuItemId> = hidden.into_iter().collect();
    ITEMS
        .iter()
        .filter(|row| !row.locked && set.contains(&row.id))
        .map(|row| row.id.name().to_owned())
        .collect()
}

/// `ContextMenuItems.ApplyHidden(settings, hidden)`: stores `hidden` (without locked ids) in
/// `settings.hidden_context_menu_items` as [`to_stored`] and mirrors it into the legacy flags: `show_zoom_menu_items`
/// is false only when `Fit`, `ZoomToLevel` and `ZoomSubmenu` are all hidden; `show_folder_menu_items` is false only
/// when `OpenFolder`, `NextFolder` and `PreviousFolder` are all hidden; `show_recycle_menu_item` is false exactly when
/// `MoveToRecycleBin` is hidden.
pub fn apply_hidden(settings: &mut AppSettings, hidden: impl IntoIterator<Item = ContextMenuItemId>) {
    let set: HashSet<ContextMenuItemId> = hidden.into_iter().filter(|id| !item_info(*id).locked).collect();
    settings.hidden_context_menu_items = Some(to_stored(set.iter().copied()).into_iter().map(Some).collect());
    settings.show_zoom_menu_items =
        !(set.contains(&Id::Fit) && set.contains(&Id::ZoomToLevel) && set.contains(&Id::ZoomSubmenu));
    settings.show_folder_menu_items =
        !(set.contains(&Id::OpenFolder) && set.contains(&Id::NextFolder) && set.contains(&Id::PreviousFolder));
    settings.show_recycle_menu_item = !set.contains(&Id::MoveToRecycleBin);
}

/// `ContextMenuItems.Compute(hidden, context, parent)`: the visibility rule for one menu level (`parent` `None` = top
/// level, else that submenu's children).
///
/// An item shows when it is not hidden by the user (the locked `Settings` item always shows), its context allows it
/// (`needs_photo` needs `context.has_photo`) and, for an item that has children, at least one child shows. Groups are
/// taken in order of first appearance in [`ITEMS`]; a separator entry (`item: None`, `group` = the group about to
/// start) is added before a group only when an earlier group had visible items.
pub fn compute(
    hidden: &HashSet<ContextMenuItemId>,
    context: ContextMenuContext,
    parent: Option<ContextMenuItemId>,
) -> ContextMenuLayout {
    let mut groups: Vec<i32> = Vec::new();
    for row in ITEMS.iter().filter(|row| row.parent == parent) {
        if !groups.contains(&row.group) {
            groups.push(row.group);
        }
    }
    let mut entries = Vec::new();
    let mut emitted_any = false;
    for group in groups {
        let visible: Vec<ContextMenuItemId> = ITEMS
            .iter()
            .filter(|row| row.parent == parent && row.group == group && item_shows(row, hidden, context))
            .map(|row| row.id)
            .collect();
        if visible.is_empty() {
            continue;
        }
        if emitted_any {
            entries.push(ContextMenuEntry { item: None, group });
        }
        entries.extend(visible.into_iter().map(|id| ContextMenuEntry { item: Some(id), group }));
        emitted_any = true;
    }
    ContextMenuLayout { entries }
}

fn item_shows(item: &ContextMenuItemInfo, hidden: &HashSet<ContextMenuItemId>, context: ContextMenuContext) -> bool {
    if !item.locked && hidden.contains(&item.id) {
        return false;
    }
    if item.needs_photo && !context.has_photo {
        return false;
    }
    let mut children = ITEMS.iter().filter(|row| row.parent == Some(item.id)).peekable();
    if children.peek().is_some() {
        // A submenu with nothing left to show would be an empty arrow: collapse it too.
        return children.any(|child| item_shows(child, hidden, context));
    }
    true
}

/// `char.IsWhiteSpace`: Unicode White_Space plus U+001C..U+001F (which .NET also treats as white space).
fn is_net_whitespace(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

fn trim_net(s: &str) -> &str {
    s.trim_matches(is_net_whitespace)
}

/// Emulates `Enum.TryParse<ContextMenuItemId>(text, ignoreCase: false)` followed by `Enum.IsDefined`.
fn try_parse_enum(text: &str) -> Option<ContextMenuItemId> {
    let text = trim_net(text);
    let first = text.chars().next()?;
    let value = if first.is_ascii_digit() || first == '+' || first == '-' {
        // Int32.TryParse(AllowLeadingSign): .NET tolerates trailing NULs.
        text.trim_end_matches('\0').parse::<i32>().ok()?
    } else {
        let mut value = 0i32;
        for part in text.split(',') {
            let id = ContextMenuItemId::ALL.iter().find(|id| id.name() == trim_net(part))?;
            value |= *id as i32;
        }
        value
    };
    ContextMenuItemId::from_value(value)
}
