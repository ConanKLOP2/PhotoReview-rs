//! Port of `PhotoReview.App.Shared/Presenters/SecondaryDialogPresenters.cs` (C# baseline `d92d6aae`, 160 lines).
//!
//! WP-35 G0: four presenters for secondary windows, all without any window type:
//! - [`ClickZoomCustomPresenter`]: the "Click zoom level > Custom..." prompt (one integer in the click zoom range).
//! - [`DiagnosticsPresenter`] (+ [`DiagnosticsDisplay`]): a metrics snapshot as the 15 texts of the diagnostics dialog.
//! - [`BatchReviewPresenter`]: the lines of the batch-review confirmation list.
//! - [`SkippedFilesPresenter`]: the title, intro, close text and lines of the "skipped files" list (ADR 0007 section 3).
//!
//! Texts come from `pr_l10n` only (decision D-W8-4 of `docs/WAVE-8-PLAN.md`): every function takes the
//! [`Localizer`] it should use (decision D-W8-10c-3), so tests choose the language without global state. Number
//! formatting in the current UI culture goes through [`NumberFormatting`] (decision D-W8-10c-2).
//!
//! Contract: `contracts/W8-10c.md`. Stub: `todo!()` bodies only (decision D11).

#![allow(unused_variables, dead_code)]

use std::path::PathBuf;

use pr_core::abstractions::{BatchReviewItem, SkippedEntry};
use pr_core::catalog::explorer_types::{ExplorerGroupState, ExplorerOrderStatus, ExplorerViewSnapshot};
use pr_core::diagnostics::metrics::ReviewMetricsSnapshot;
use pr_l10n::localizer::Localizer;

/// The number formats the C# presenters take from `CultureInfo.CurrentCulture`.
///
/// - `grouped_i64` / `grouped_f64`: .NET `ToString("N0", culture)`: culture group separator and culture negative sign,
///   no decimals; `grouped_f64` rounds the double first (half away from zero on the exact decimal value, the same rule as
///   `format_fixed` in `viewport::types`, W8-01a).
/// - `percent_one_decimal`: .NET `string.Format(culture, "{0:P1}", ratio)`: `ratio * 100`, one decimal, the culture's
///   percent pattern and symbol.
///
/// `pr_l10n::template::NumberCulture` has only sign and decimal separator today, so the production implementation is
/// written by the composition root from the Windows user locale (open question OQ-1). Tests use a hand-written en-US
/// implementation.
pub trait NumberFormatting {
    fn grouped_i64(&self, value: i64) -> String;
    fn grouped_f64(&self, value: f64) -> String;
    fn percent_one_decimal(&self, ratio: f64) -> String;
}

/// C# `ClickZoomCustomPresenter`: one whole number in `[MIN_CLICK_ZOOM_PERCENT, MAX_CLICK_ZOOM_PERCENT]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClickZoomCustomPresenter {
    initial_text: String,
    value: i32,
}

impl ClickZoomCustomPresenter {
    /// The box starts with the current value written in invariant digits (`150`).
    pub fn new(current_percent: i32) -> Self {
        todo!()
    }

    /// The text the box starts with (C# `InitialText`).
    pub fn initial_text(&self) -> &str {
        todo!()
    }

    /// The last accepted value (C# `Value`). `0` until the first accepted text (C# `int` default); a refusal keeps it.
    pub fn value(&self) -> i32 {
        todo!()
    }

    /// C# `TryAccept(string? text)`. `Ok(())` is C#'s `null` return (the value is stored in [`Self::value`]);
    /// `Err(message)` is the localized refusal `dialog.settingsInvalid.clickZoomPercent` with `{min}` and `{max}` filled
    /// from `MIN_CLICK_ZOOM_PERCENT` / `MAX_CLICK_ZOOM_PERCENT`. `None` (C# `null` text) is a refusal too.
    pub fn try_accept(&mut self, localizer: &Localizer, text: Option<&str>) -> Result<(), String> {
        todo!()
    }
}

/// The 15 value texts of the diagnostics dialog, in the row order of its label column (C# `DiagnosticsDisplay`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsDisplay {
    pub cache_hits: String,
    pub cache_misses: String,
    pub hit_rate: String,
    pub source_bytes: String,
    pub source_reads: String,
    pub decode_time: String,
    pub present_latency: String,
    pub explorer_order: String,
    pub explorer_metadata: String,
    pub source_opens: String,
    pub stat_count: String,
    pub session_writes: String,
    pub decoder_fallbacks: String,
    pub present_histogram: String,
    pub cross_thread_presents: String,
}

/// C# `DiagnosticsPresenter` (static class). Read-only dialog: no state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticsPresenter;

impl DiagnosticsPresenter {
    /// C# `Build(ReviewMetricsSnapshot, ExplorerViewSnapshot? = null)`. `explorer: None` is C#'s `null` (not queried).
    ///
    /// Counters use `numbers.grouped_i64`; the rest follows the field table in the contract (semantics 6, traps T3 to T11).
    pub fn build(
        snapshot: &ReviewMetricsSnapshot,
        explorer: Option<&ExplorerViewSnapshot>,
        localizer: &Localizer,
        numbers: &dyn NumberFormatting,
    ) -> DiagnosticsDisplay {
        todo!()
    }

    /// C# `ReasonText(string reason)`: a translated sentence for an `ExplorerReason` wire text (`code|detail|...`).
    /// An unknown code is `diag.explorerReason.unknown` with the WHOLE input text.
    pub fn reason_text(reason: &str, localizer: &Localizer) -> String {
        todo!()
    }

    /// C# `StatusText(ExplorerOrderStatus)`: one catalog text per variant (`enum.explorerOrderStatus.*`).
    pub fn status_text(status: ExplorerOrderStatus, localizer: &Localizer) -> String {
        todo!()
    }

    /// The English member name of an `ExplorerGroupState` as C# `ToString()` writes it (`"None"`, `"Active"`,
    /// `"Unknown"`). Used for `diagnostics.explorerMetadata`'s `{groupState}` (the C# enum is passed unformatted).
    pub fn group_state_name(state: ExplorerGroupState) -> &'static str {
        todo!()
    }
}

/// C# `Path.GetFileName(string)` for the paths the app shows: the text after the last `\` or `/`; a path without a
/// separator is returned as it is; a path ending in a separator gives `""`. Never a `Path` method (see contract trap T5).
pub fn file_name(path: &str) -> &str {
    todo!()
}

/// C# `BatchReviewPresenter` (static class).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchReviewPresenter;

impl BatchReviewPresenter {
    /// One line per item, in order (C# `BuildLines`).
    pub fn build_lines(
        items: &[BatchReviewItem],
        localizer: &Localizer,
        numbers: &dyn NumberFormatting,
    ) -> Vec<String> {
        todo!()
    }

    /// One list line: `batchReview.item` when the size is known, `batchReview.itemMissing` otherwise (C# `BuildItem`).
    pub fn build_item(item: &BatchReviewItem, localizer: &Localizer, numbers: &dyn NumberFormatting) -> String {
        todo!()
    }

    /// The heading under the title, `batchReview.summary` (plural by `count`, C# `Summary(int)`).
    pub fn summary(count: i64, localizer: &Localizer) -> String {
        todo!()
    }

    /// The path-only form (sizes unknown): every item has `length: None` (C# `FromPaths`).
    pub fn from_paths(paths: &[PathBuf]) -> Vec<BatchReviewItem> {
        todo!()
    }
}

/// C# `SkippedFilesPresenter` (static class).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkippedFilesPresenter;

impl SkippedFilesPresenter {
    /// `main.skipped.window.title` (C# `Title`).
    pub fn title(localizer: &Localizer) -> String {
        todo!()
    }

    /// `main.skipped.window.intro` (C# `Intro`).
    pub fn intro(localizer: &Localizer) -> String {
        todo!()
    }

    /// `main.skipped.window.close` (C# `CloseText`).
    pub fn close_text(localizer: &Localizer) -> String {
        todo!()
    }

    /// One line per entry, in order (C# `BuildLines`).
    pub fn build_lines(entries: &[SkippedEntry], localizer: &Localizer) -> Vec<String> {
        todo!()
    }

    /// `skipped.entry` with the full path and the reason sentence (C# `FormatEntry`). The OS message stays verbatim
    /// inside `skipped.reason.fileUnreadable` or `skipped.reason.listingInterrupted`.
    pub fn format_entry(entry: &SkippedEntry, localizer: &Localizer) -> String {
        todo!()
    }
}
