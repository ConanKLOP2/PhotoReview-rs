//! Port of `PhotoReview.App.Shared/Input/ShortcutRouter.cs` (300 lines, baseline `d92d6aae`). K-* rows of `docs/PARITY.md`.
//!
//! Pure: no `windows` crate, no `unsafe`, no clock, no I/O. One key event becomes at most one `ReviewCommand`: the first
//! match in the precedence order of contract W8-05d, section "Precedence". Key names go through
//! `services::shortcut_key_name::try_parse` (W8-04c, port of `Services/ShortcutKeyName.cs`).
//!
//! Lead-owned file `input/mod.rs` gets `pub mod shortcut_router;` when this unit merges (contract W8-05d, "Owned files").

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use pr_core::settings::shortcuts::{ReviewAction, ShortcutMappings};

use crate::input::key_id::KeyId;
use crate::input::primitives::KeyModifiers;
use crate::input::review_command::ReviewCommand;

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
        todo!()
    }
}

/// C# `_actionKeys` entry `(KeyId Key, int Index)`. `index` is the position in the FULL `Actions` list, so an action
/// with an empty or unparseable shortcut still takes its index (contract trap T6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionBinding {
    pub key: KeyId,
    pub index: usize,
}

/// The 27 single-key bindings of C# (`_fullscreenKey` ... `_refreshKey`). `None` = disabled (empty or unparseable).
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
        todo!()
    }

    /// C# `new ShortcutRouter(settings)`: [`ShortcutRouter::new`] followed by [`ShortcutRouter::rebuild`].
    pub fn from_settings(shortcuts: &ShortcutMappings, actions: &[ReviewAction]) -> Self {
        todo!()
    }

    /// C# `Rebuild(AppSettings)`: every binding and every action binding is recomputed. No stale key survives, and
    /// an action list shorter than before drops the old indices.
    pub fn rebuild(&mut self, shortcuts: &ShortcutMappings, actions: &[ReviewAction]) {
        todo!()
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
        todo!()
    }
}

/// C# `ParseKey(string?)`: blank or whitespace text is `None`; otherwise `ShortcutKeyName.TryParse`, which also rejects
/// reserved names (`None`, `Escape`, `Tab`, `System`, modifiers, IME), numbers, and comma lists. Never panics.
fn parse_key(text: Option<&str>) -> Option<KeyId> {
    todo!()
}
