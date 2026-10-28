//! Port of `PhotoReview.App.Shared/Coordinators/ImagePresenter.cs` lines 714 to 770 (baseline d92d6aae):
//! `RemoveOrDegrade` (719 to 730, belongs to the plan's 07j window but is only used here) and
//! `RemoveMissingCatalogItemAsync` (735 to 770). Contract: `contracts/W8-07k.md`. Target:
//! `crates/pr-app/src/coordinators/presenter/missing.rs` (`coordinators::presenter::missing`).
//!
//! The C# method is one `while (true)` loop with an `await` (the stat of the NEXT photo) per round. The Rust form is one
//! `MissingRun` (the loop variables) and a step function: `missing_step` does a round up to the stat request, `on_missing_next_stat`
//! resumes it. Each resume re-checks the navigation token before it touches the catalog (INV-1).

use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::sync::CancelToken;
use pr_core::text::eq_ignore_case;

use crate::viewmodels::format::status;

use super::photo_info::{StatOutcome, StatResult};
use super::state::{ImagePresenter, PresentRequest, PresenterChanges, PresenterEnv, StatPurpose};

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
    /// The run is nested under `PresentCoreAsync` and contributes to C# `_presentsInFlight` until its continuation ends.
    pub(super) awaited_present: bool,
}

/// C# `RemoveOrDegrade(path)`: removes a missing file from the catalog. A missing member of a JPEG+RAW capture group must not
/// hide its surviving partner: the entry degrades to the other member as a standalone (group-less) entry at the same position
/// and is selected. Returns the new current index (as `ReviewCatalog::remove` does).
///
/// Steps: `index = catalog.index_of(path)`; `survivor` = the first of `catalog.find(path)?.capture_group.image_paths()` that is NOT equal to `path`
/// under `OrdinalIgnoreCase` (`pr_core::text::eq_ignore_case`), only when `index >= 0`; no survivor: `return catalog.remove(path)`;
/// else `catalog.remove(path)`, `catalog.restore(survivor, index)`, `catalog.set_current(index)`, return `index`.
pub fn remove_or_degrade(catalog: &mut ReviewCatalog, path: &str) -> i32 {
    let index = catalog.index_of(path);
    let survivor = if index >= 0 {
        catalog
            .find(path)
            .and_then(|entry| entry.capture_group.as_ref())
            .and_then(|group| group.image_paths().iter().find(|member| !eq_ignore_case(member, path)))
            .cloned()
    } else {
        None
    };
    let Some(survivor) = survivor else {
        return catalog.remove(path);
    };
    // Remove BEFORE restore (restoring first would collide with the existing group entry), restore BEFORE set_current.
    catalog.remove(path);
    catalog.restore(&survivor, index);
    catalog.set_current(index);
    index
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
        self.begin_missing_run(env, path, token, false);
        self.take_changes()
    }

    /// The start of [`remove_missing_catalog_item`](Self::remove_missing_catalog_item) for callers that are already inside a `pump` / `present` call
    /// (the changes stay accumulated in `self.changes` for that call to return, instead of being taken here).
    pub(super) fn begin_missing_run(
        &mut self,
        env: &mut PresenterEnv<'_>,
        path: &str,
        token: i64,
        awaited_present: bool,
    ) {
        self.missing_step(
            env,
            MissingRun {
                token,
                path: path.to_owned(),
                pending_next: None,
                awaited_present,
            },
        );
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
        // Any earlier run is replaced (or has ended); this one is the only run from here on.
        self.missing = None;
        if !self.clock.is_navigation_current(run.token) {
            return;
        }

        let next_index = remove_or_degrade(env.catalog, &run.path);
        if env.catalog.count() == 0 {
            // Nothing is left: reset the presentation and say so (the status is never faded out).
            self.current_photo_info = None;
            self.changes |= PresenterChanges::PHOTO_INFO;
            self.info_while_original_shown = None;
            let effects = self.zoom_detail.reset();
            self.apply_zoom_effects(effects);
            self.update_current_image(None, 0, 0, None);
            if !self.compare.clear(&env.format).is_empty() {
                self.changes |= PresenterChanges::COMPARE;
            }
            let text = status::no_images_remaining(&env.format);
            self.update_status(text, true);
            return;
        }

        let in_range = usize::try_from(next_index)
            .ok()
            .filter(|index| *index < env.catalog.count());
        let Some(next_slot) = in_range else {
            return;
        };
        let next_path = env.catalog.path_at(next_slot).to_owned();
        // Only a file that is really gone is skipped; an unreadable one is presented so its error is reported. The stat is off the UI thread and the
        // navigation token is re-checked when it comes back.
        self.submit_stat(run.token, &next_path, StatPurpose::NextForMissing, &CancelToken::none());
        self.missing = Some(MissingRun {
            pending_next: Some((next_path, next_index)),
            ..run
        });
    }

    /// The code after the C# `await StatOffUiThreadAsync(nextPath, None)`: `!is_navigation_current(token)`: the run ends. Outcome other than
    /// `Missing` (a found OR unreadable next photo): the run ends with `present(PresentRequest::new(next_index))` where `next_index` is the one
    /// computed in the round (C# `await PresentAsync(nextIndex)`; it is NOT re-read from the catalog). `Missing`: `run.path = next_path` and the
    /// next round (`missing_step`), iteratively (no recursion, R2-F-07).
    pub(super) fn on_missing_next_stat(&mut self, env: &mut PresenterEnv<'_>, token: i64, result: StatResult) {
        let waiting = matches!(&self.missing, Some(run) if run.token == token && run.pending_next.is_some());
        if !waiting {
            return;
        }
        let Some(mut run) = self.missing.take() else {
            return;
        };
        if !self.clock.is_navigation_current(token) {
            return;
        }
        let Some((next_path, next_index)) = run.pending_next.take() else {
            return;
        };
        if result.outcome != StatOutcome::Missing {
            // A found OR unreadable next photo ends the run with one normal present of the index computed in this round (it is not re-read).
            let parent = token;
            if let Err(error) = self.begin_present(env, PresentRequest::new(next_index), 0, None) {
                crate::misc::app_log::AppLog::global().error(
                    &format!("ShowImage next present failed token={token} path={next_path}"),
                    Some(&error),
                );
            } else if let Some(child) = self.navigation.as_ref().map(|nav| nav.token) {
                self.track_child_present(parent, child);
            }
            return;
        }
        // The next photo is gone too: it becomes the path the loop removes (iteratively, no recursion).
        run.path = next_path;
        self.missing_step(env, run);
    }
}
