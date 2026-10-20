//! Port of `PhotoReview.App.Shared/Coordinators/ImagePresenter.cs` lines 714 to 770 (baseline d92d6aae):
//! `RemoveOrDegrade` (719 to 730, belongs to the plan's 07j window but is only used here) and
//! `RemoveMissingCatalogItemAsync` (735 to 770). Contract: `contracts/W8-07k.md`. Target:
//! `crates/pr-app/src/coordinators/presenter/missing.rs` (`coordinators::presenter::missing`).
//!
//! The C# method is one `while (true)` loop with an `await` (the stat of the NEXT photo) per round. The Rust form is one
//! `MissingRun` (the loop variables) and a step function: `missing_step` does a round up to the stat request, `on_missing_next_stat`
//! resumes it. Each resume re-checks the navigation token before it touches the catalog (INV-1).
// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]

use pr_core::catalog::review_catalog::ReviewCatalog;

use super::photo_info::StatResult;
use super::state::{ImagePresenter, PresenterChanges, PresenterEnv};

/// The loop state of one `RemoveMissingCatalogItemAsync(path, token)` call (C# parameters, reassigned by the loop:
/// `path = nextPath`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MissingRun {
    /// The navigation token the run belongs to. Every step first checks `clock.is_navigation_current(token)`.
    pub(super) token: i64,
    /// The path the loop currently removes; the stat in flight is the one of the photo AFTER it.
    pub(super) path: String,
    /// The photo whose stat is pending: C# `(nextPath, nextIndex)`; `None` between rounds.
    pub(super) pending_next: Option<(String, i32)>,
}

/// C# `RemoveOrDegrade(path)`: removes a missing file from the catalog. A missing member of a JPEG+RAW capture group must not
/// hide its surviving partner: the entry degrades to the other member as a standalone (group-less) entry at the same position
/// and is selected. Returns the new current index (as `ReviewCatalog::remove` does).
///
/// Steps: `index = catalog.index_of(path)`; `survivor` = the first of `catalog.find(path)?.capture_group.image_paths()` that is NOT equal to `path`
/// under `OrdinalIgnoreCase` (`pr_core::text::eq_ignore_case`), only when `index >= 0`; no survivor: `return catalog.remove(path)`;
/// else `catalog.remove(path)`, `catalog.restore(survivor, index)`, `catalog.set_current(index)`, return `index`.
pub fn remove_or_degrade(catalog: &mut ReviewCatalog, path: &str) -> i32 {
    todo!()
}

impl ImagePresenter {
    /// C# `RemoveMissingCatalogItemAsync(path, token)` (public: also called with a token the caller took itself). Starts the loop.
    /// A stale token returns at once with the catalog untouched. Replaces any run already pending (its token is never newer than
    /// this one in any reachable call order; a run of an older token would stop at its first token check anyway).
    pub fn remove_missing_catalog_item(
        &mut self,
        env: &mut PresenterEnv<'_>,
        path: &str,
        token: i64,
    ) -> PresenterChanges {
        todo!()
    }

    /// The start of [`remove_missing_catalog_item`](Self::remove_missing_catalog_item) for callers that are already inside a `pump` / `present` call
    /// (the changes stay accumulated in `self.changes` for that call to return, instead of being taken here).
    pub(super) fn begin_missing_run(&mut self, env: &mut PresenterEnv<'_>, path: &str, token: i64) {
        todo!()
    }

    /// One round of the C# loop up to its `await`:
    /// 1. `!clock.is_navigation_current(run.token)`: the run ends (nothing touched).
    /// 2. `next_index = remove_or_degrade(env.catalog, &run.path)`.
    /// 3. `env.catalog.count() == 0`: `current_photo_info = None`, `info_while_original_shown = None`, `zoom_detail.reset()` (apply its effects),
    ///    `update_current_image(None, 0, 0, None)`, `compare.clear(ctx)`, `update_status(status::no_images_remaining(ctx), true)`; the run ends.
    /// 4. `next_index < 0 || next_index >= count`: the run ends.
    /// 5. `next_path = catalog.path_at(next_index)`; the stat of `next_path` goes to the stat worker (token = never cancelled, C# `CancellationToken.None`);
    ///    its outcome comes back as `PresenterEvent::Stat { purpose: NextForMissing }`.
    pub(super) fn missing_step(&mut self, env: &mut PresenterEnv<'_>, run: MissingRun) {
        todo!()
    }

    /// The code after the C# `await StatOffUiThreadAsync(nextPath, None)`: `!is_navigation_current(token)`: the run ends. Outcome other than
    /// `Missing` (a found OR unreadable next photo): the run ends with `present(PresentRequest::new(next_index))` where `next_index` is the one
    /// computed in the round (C# `await PresentAsync(nextIndex)`; it is NOT re-read from the catalog). `Missing`: `run.path = next_path` and the
    /// next round (`missing_step`), iteratively (no recursion, R2-F-07).
    pub(super) fn on_missing_next_stat(&mut self, env: &mut PresenterEnv<'_>, token: i64, result: StatResult) {
        todo!()
    }
}
