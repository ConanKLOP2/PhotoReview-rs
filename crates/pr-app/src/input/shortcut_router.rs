//! Port of `PhotoReview.App.Shared/Input/ShortcutRouter.cs` (300 lines, baseline `d92d6aae`). K-* rows of `docs/PARITY.md`.
//!
//! Pure: no `windows` crate, no `unsafe`, no clock, no I/O. One key event becomes at most one `ReviewCommand`: the first
//! match in the precedence order of contract W8-05d, section "Precedence". Key names go through
//! `services::shortcut_key_name::try_parse` (W8-04c, port of `Services/ShortcutKeyName.cs`).
//!
//! Lead-owned file `input/mod.rs` gets `pub mod shortcut_router;` when this unit merges (contract W8-05d, "Owned files").

use pr_core::settings::shortcuts::{ReviewAction, ShortcutMappings};

use crate::input::key_id::KeyId;
use crate::input::primitives::KeyModifiers;
use crate::input::review_command::ReviewCommand;
use crate::services::shortcut_key_name::try_parse;

/// The per-event flags of C# `TryResolve`: `isFullscreen`, `hasImage`, `hasComparePair`, `isCompareVisible`,
/// `hasCapturePair`. Built with [`RouteState::new`], which applies the C# defaults of the optional flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteState {
    pub is_fullscreen: bool,
    pub has_image: bool,
    pub has_compare_pair: bool,
    pub is_compare_visible: bool,
    pub has_capture_pair: bool,
}

impl RouteState {
    /// C# defaults of the optional flags: `hasComparePair = true`, `isCompareVisible = false`, `hasCapturePair = false`.
    /// Note: the compare default is `true`, not `false` (a `Default` derive would be wrong).
    pub fn new(is_fullscreen: bool, has_image: bool) -> Self {
        Self {
            is_fullscreen,
            has_image,
            has_compare_pair: true,
            is_compare_visible: false,
            has_capture_pair: false,
        }
    }
}

/// C# `_actionKeys` entry `(KeyId Key, int Index)`. `index` is the position in the FULL `Actions` list, so an action
/// with an empty or unparseable shortcut still takes its index (contract trap T6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionBinding {
    pub key: KeyId,
    pub index: usize,
}

/// The single-key bindings of C# (`_fullscreenKey` ... `_renameFileKey`). `None` = disabled (empty or unparseable).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Bindings {
    fullscreen: Option<KeyId>,
    next_folder: Option<KeyId>,
    previous_folder: Option<KeyId>,
    first_image: Option<KeyId>,
    last_image: Option<KeyId>,
    toggle_info_overlay: Option<KeyId>,
    zoom_actual_size: Option<KeyId>,
    undo: Option<KeyId>,
    compare: Option<KeyId>,
    recycle: Option<KeyId>,
    skip: Option<KeyId>,
    toggle_fit: Option<KeyId>,
    zoom_in: Option<KeyId>,
    zoom_out: Option<KeyId>,
    next: Option<KeyId>,
    previous: Option<KeyId>,
    move_to_folder: Option<KeyId>,
    copy_to_folder: Option<KeyId>,
    click_zoom: Option<KeyId>,
    fit_width: Option<KeyId>,
    fit_width2: Option<KeyId>,
    fit_height: Option<KeyId>,
    toggle_keep_zoom: Option<KeyId>,
    open_folder: Option<KeyId>,
    custom_zoom: Option<KeyId>,
    toggle_capture_member: Option<KeyId>,
    refresh: Option<KeyId>,
    rename_file: Option<KeyId>,
}

/// C# `ShortcutRouter`: the key table built from the settings. Holds no UI state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShortcutRouter {
    bindings: Bindings,
    actions: Vec<ActionBinding>,
}

impl ShortcutRouter {
    /// C# `new ShortcutRouter()` (no settings): nothing is bound. `Escape` still closes the window (it needs no binding).
    pub fn new() -> Self {
        Self::default()
    }

    /// C# `new ShortcutRouter(settings)`: [`ShortcutRouter::new`] followed by [`ShortcutRouter::rebuild`].
    pub fn from_settings(shortcuts: &ShortcutMappings, actions: &[ReviewAction]) -> Self {
        let mut router = Self::new();
        router.rebuild(shortcuts, actions);
        router
    }

    /// C# `Rebuild(AppSettings)`: every binding and every action binding is recomputed. No stale key survives, and
    /// an action list shorter than before drops the old indices.
    pub fn rebuild(&mut self, shortcuts: &ShortcutMappings, actions: &[ReviewAction]) {
        let p = |text: &str| parse_key(Some(text));
        self.bindings = Bindings {
            fullscreen: p(&shortcuts.fullscreen),
            next_folder: p(&shortcuts.next_folder),
            previous_folder: p(&shortcuts.previous_folder),
            first_image: p(&shortcuts.first_image),
            last_image: p(&shortcuts.last_image),
            toggle_info_overlay: p(&shortcuts.toggle_info_overlay),
            zoom_actual_size: p(&shortcuts.zoom_actual_size),
            undo: p(&shortcuts.undo),
            compare: p(&shortcuts.compare),
            recycle: p(&shortcuts.send_to_recycle_bin),
            skip: p(&shortcuts.skip),
            toggle_fit: p(&shortcuts.toggle_fit),
            zoom_in: p(&shortcuts.zoom_in),
            zoom_out: p(&shortcuts.zoom_out),
            next: p(&shortcuts.next),
            previous: p(&shortcuts.previous),
            move_to_folder: p(&shortcuts.move_to_folder),
            copy_to_folder: p(&shortcuts.copy_to_folder),
            click_zoom: p(&shortcuts.click_zoom),
            fit_width: p(&shortcuts.fit_width),
            fit_width2: p(&shortcuts.fit_width2),
            fit_height: p(&shortcuts.fit_height),
            toggle_keep_zoom: p(&shortcuts.toggle_keep_zoom),
            open_folder: p(&shortcuts.open_folder),
            custom_zoom: p(&shortcuts.custom_zoom),
            toggle_capture_member: p(&shortcuts.toggle_capture_member),
            refresh: p(&shortcuts.refresh),
            rename_file: p(&shortcuts.rename_file),
        };
        // The index is the position in the FULL list: a skipped entry does not renumber the later ones.
        self.actions = actions
            .iter()
            .enumerate()
            .filter_map(|(index, action)| p(&action.shortcut).map(|key| ActionBinding { key, index }))
            .collect();
    }

    /// C# `TryResolve`. `key` is the pressed key, `system_key` the `Key.System` payload (`KeyId::None` when the event
    /// is not a system key), `modifiers` the raw ModifierKeys bit set. Returns the first command of the precedence
    /// order, or `None`. Only the fullscreen binding looks at `system_key`; every other rule uses `key`.
    pub fn try_resolve(
        &self,
        key: KeyId,
        system_key: KeyId,
        modifiers: KeyModifiers,
        state: RouteState,
    ) -> Option<ReviewCommand> {
        let b = &self.bindings;
        let pressed = if key == KeyId::SYSTEM { system_key } else { key };
        let is = |binding: Option<KeyId>| binding == Some(key);
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        let bare = modifiers == KeyModifiers::NONE;

        if b.fullscreen == Some(pressed) {
            return Some(ReviewCommand::FULLSCREEN);
        }
        if key == KeyId::ESCAPE {
            return Some(if state.is_fullscreen {
                ReviewCommand::EXIT_FULLSCREEN
            } else {
                ReviewCommand::CLOSE
            });
        }
        if is(b.next_folder) {
            return Some(ReviewCommand::NEXT_FOLDER);
        }
        if is(b.previous_folder) {
            return Some(ReviewCommand::PREVIOUS_FOLDER);
        }
        if is(b.first_image) {
            return state.has_image.then_some(ReviewCommand::FIRST_IMAGE);
        }
        if is(b.last_image) {
            return state.has_image.then_some(ReviewCommand::LAST_IMAGE);
        }
        if is(b.undo) && ctrl {
            return Some(ReviewCommand::UNDO);
        }
        if is(b.toggle_info_overlay) {
            return Some(ReviewCommand::TOGGLE_INFO_OVERLAY);
        }
        if is(b.toggle_keep_zoom) {
            return Some(ReviewCommand::TOGGLE_KEEP_ZOOM);
        }
        if is(b.open_folder) && ctrl {
            return Some(ReviewCommand::OPEN_FOLDER);
        }
        if !state.has_image {
            return None;
        }
        if is(b.compare) {
            let nothing_to_toggle = !state.is_compare_visible && !state.has_compare_pair;
            return (!nothing_to_toggle).then_some(ReviewCommand::TOGGLE_COMPARE);
        }
        if is(b.toggle_capture_member) && state.has_capture_pair && !state.is_compare_visible {
            return Some(ReviewCommand::TOGGLE_CAPTURE_MEMBER);
        }
        if bare {
            if let Some(binding) = self.actions.iter().find(|a| a.key == key) {
                // Action counts are tiny; saturate rather than wrap on the impossible overflow.
                return Some(ReviewCommand::action(i32::try_from(binding.index).unwrap_or(i32::MAX)));
            }
            if is(b.recycle) {
                return Some(ReviewCommand::RECYCLE);
            }
            if is(b.rename_file) {
                return Some(ReviewCommand::RENAME_FILE);
            }
        }
        let simple: [(Option<KeyId>, ReviewCommand); 13] = [
            (b.skip, ReviewCommand::SKIP),
            (b.toggle_fit, ReviewCommand::TOGGLE_FIT),
            (b.zoom_in, ReviewCommand::ZOOM_IN),
            (b.zoom_out, ReviewCommand::ZOOM_OUT),
            (b.zoom_actual_size, ReviewCommand::ZOOM_ACTUAL_SIZE),
            (b.click_zoom, ReviewCommand::CLICK_ZOOM),
            (b.fit_width, ReviewCommand::FIT_WIDTH),
            (b.fit_width2, ReviewCommand::FIT_WIDTH2),
            (b.fit_height, ReviewCommand::FIT_HEIGHT),
            (b.custom_zoom, ReviewCommand::CUSTOM_ZOOM),
            (b.refresh, ReviewCommand::REFRESH),
            (b.next, ReviewCommand::NEXT),
            (b.previous, ReviewCommand::PREVIOUS),
        ];
        for (binding, command) in simple.iter() {
            if is(*binding) {
                return Some(*command);
            }
        }
        if !ctrl {
            let force_picker = modifiers.contains(KeyModifiers::SHIFT);
            if is(b.move_to_folder) {
                return Some(ReviewCommand::move_to_folder(force_picker));
            }
            if is(b.copy_to_folder) {
                return Some(ReviewCommand::copy_to_folder(force_picker));
            }
        }
        None
    }
}

/// C# `ParseKey(string?)`: blank or whitespace text is `None`; otherwise `ShortcutKeyName.TryParse`, which also rejects
/// reserved names (`None`, `Escape`, `Tab`, `System`, modifiers, IME), numbers, and comma lists. Never panics.
fn parse_key(text: Option<&str>) -> Option<KeyId> {
    match text {
        Some(t) if !t.trim().is_empty() => try_parse(Some(t)),
        _ => None,
    }
}
