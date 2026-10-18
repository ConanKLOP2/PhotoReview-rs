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
//! Contract: `contracts/W8-10c.md`.

use std::path::PathBuf;

use pr_core::abstractions::{BatchReviewItem, SkippedEntry, SkippedKind};
use pr_core::catalog::explorer_reason::{
    self, CANCELED, COM_CALL_FAILED, DUPLICATE_ITEM, EMPTY_PATH, EMPTY_VIEW, INCOMPLETE_SNAPSHOT, NATIVE_VIEW_FAILED,
    NO_MATCHING_WINDOW, OUTSIDE_FOLDER, QUERY_FAILED, SHELL_UNAVAILABLE, TIMEOUT,
};
use pr_core::catalog::explorer_types::{ExplorerGroupState, ExplorerOrderStatus, ExplorerViewSnapshot};
use pr_core::diagnostics::metrics::ReviewMetricsSnapshot;
use pr_core::settings::app_settings::{MAX_CLICK_ZOOM_PERCENT, MIN_CLICK_ZOOM_PERCENT};
use pr_l10n::keys;
use pr_l10n::localizer::Localizer;
use pr_l10n::template::LocArg;

/// Catalog key of the plural group of the batch-review heading (`.one` / `.other` are picked by count).
const BATCH_REVIEW_SUMMARY: &str = "batchReview.summary";

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

/// Characters .NET `NumberStyles.Integer` allows as white space: U+0009 to U+000D and U+0020 only (trap T1; not
/// `char::is_whitespace`, which also takes U+00A0).
fn is_dotnet_white_space(c: char) -> bool {
    matches!(c, '\u{9}'..='\u{d}' | ' ')
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
        Self {
            initial_text: current_percent.to_string(),
            value: 0,
        }
    }

    /// The text the box starts with (C# `InitialText`).
    pub fn initial_text(&self) -> &str {
        &self.initial_text
    }

    /// The last accepted value (C# `Value`). `0` until the first accepted text (C# `int` default); a refusal keeps it.
    pub fn value(&self) -> i32 {
        self.value
    }

    /// C# `TryAccept(string? text)`. `Ok(())` is C#'s `null` return (the value is stored in [`Self::value`]);
    /// `Err(message)` is the localized refusal `dialog.settingsInvalid.clickZoomPercent` with `{min}` and `{max}` filled
    /// from `MIN_CLICK_ZOOM_PERCENT` / `MAX_CLICK_ZOOM_PERCENT`. `None` (C# `null` text) is a refusal too.
    pub fn try_accept(&mut self, localizer: &Localizer, text: Option<&str>) -> Result<(), String> {
        let accepted = text
            .map(|t| t.trim_matches(is_dotnet_white_space))
            .and_then(|t| t.parse::<i32>().ok())
            .filter(|v| (MIN_CLICK_ZOOM_PERCENT..=MAX_CLICK_ZOOM_PERCENT).contains(v));
        match accepted {
            Some(value) => {
                self.value = value;
                Ok(())
            }
            None => Err(localizer.format(
                keys::DIALOG_SETTINGS_INVALID_CLICK_ZOOM_PERCENT,
                &[
                    LocArg::new("min", MIN_CLICK_ZOOM_PERCENT.to_string()),
                    LocArg::new("max", MAX_CLICK_ZOOM_PERCENT.to_string()),
                ],
            )),
        }
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
        let not_available = localizer.format(keys::DIAGNOSTICS_NOT_AVAILABLE, &[]);
        let separator = localizer.get(keys::DIAG_LIST_SEPARATOR);

        // T3: the zero test uses the sum (wrapping, like C# unchecked long).
        let total = snapshot.cache_hits.wrapping_add(snapshot.cache_misses);
        let hit_rate = if total == 0 {
            not_available.clone()
        } else {
            // T4: the division is done in f64.
            numbers.percent_one_decimal(snapshot.cache_hits as f64 / total as f64)
        };

        // T11: the zero test and the division use `presented_images`.
        let present_latency = if snapshot.presented_images == 0 {
            not_available.clone()
        } else {
            localizer.format(
                keys::DIAGNOSTICS_VALUE_PRESENT_LATENCY,
                &[
                    LocArg::new(
                        "average",
                        numbers.grouped_f64(snapshot.present_milliseconds as f64 / snapshot.presented_images as f64),
                    ),
                    LocArg::new("count", numbers.grouped_i64(snapshot.presented_images)),
                ],
            )
        };

        // T7: `None` is "not queried" here, but `N/A` in explorer_metadata below.
        let explorer_order = match explorer {
            None => localizer.format(keys::DIAGNOSTICS_EXPLORER_ORDER_NOT_QUERIED, &[]),
            Some(view) => {
                // T8: `Some("")` is a reason (goes through reason_text), not an absent one.
                let detail = match &view.reason {
                    Some(reason) => Self::reason_text(reason, localizer),
                    None => localizer.format(
                        keys::DIAGNOSTICS_EXPLORER_ORDER_ITEM_COUNT,
                        &[LocArg::new("count", view.ordered_paths.len().to_string())],
                    ),
                };
                localizer.format(
                    keys::DIAGNOSTICS_EXPLORER_ORDER_STATUS,
                    &[
                        LocArg::new("status", Self::status_text(view.status, localizer)),
                        LocArg::new("detail", detail),
                    ],
                )
            }
        };

        let explorer_metadata = match explorer {
            None => not_available.clone(),
            Some(view) => localizer.format(
                keys::DIAGNOSTICS_EXPLORER_METADATA,
                &[
                    LocArg::new("count", view.sort_columns.len().to_string()),
                    LocArg::new("groupState", Self::group_state_name(view.group_state)),
                ],
            ),
        };

        // T6 and T10: a zero count is the literal "0"; the total is grouped, the per-entry count is raw digits.
        let source_opens = if snapshot.source_open_count == 0 {
            "0".to_string()
        } else {
            let top = snapshot
                .top_source_opens
                .iter()
                .map(|entry| {
                    localizer.format(
                        keys::DIAG_SOURCE_OPENS_ENTRY,
                        &[
                            LocArg::new("name", file_name(&entry.path)),
                            LocArg::new("count", entry.count.to_string()),
                        ],
                    )
                })
                .collect::<Vec<String>>()
                .join(separator.as_str());
            localizer.format(
                keys::DIAG_SOURCE_OPENS_SUMMARY,
                &[
                    LocArg::new("total", numbers.grouped_i64(snapshot.source_open_count)),
                    LocArg::new("top", top),
                ],
            )
        };

        let present_histogram = if snapshot.present_histogram.is_empty() {
            not_available.clone()
        } else {
            snapshot
                .present_histogram
                .iter()
                .map(|bucket| {
                    localizer.format(
                        keys::DIAG_HISTOGRAM_BUCKET,
                        &[
                            LocArg::new("label", bucket.label.clone()),
                            LocArg::new("count", numbers.grouped_i64(bucket.count)),
                        ],
                    )
                })
                .collect::<Vec<String>>()
                .join(separator.as_str())
        };

        DiagnosticsDisplay {
            cache_hits: numbers.grouped_i64(snapshot.cache_hits),
            cache_misses: numbers.grouped_i64(snapshot.cache_misses),
            hit_rate,
            source_bytes: localizer.format(
                keys::DIAGNOSTICS_VALUE_BYTES,
                &[LocArg::new("value", numbers.grouped_i64(snapshot.source_bytes_read))],
            ),
            source_reads: numbers.grouped_i64(snapshot.source_reads),
            decode_time: localizer.format(
                keys::UNIT_MILLISECONDS,
                &[LocArg::new("value", numbers.grouped_i64(snapshot.decode_milliseconds))],
            ),
            present_latency,
            explorer_order,
            explorer_metadata,
            source_opens,
            stat_count: numbers.grouped_i64(snapshot.stat_count),
            session_writes: numbers.grouped_i64(snapshot.session_write_count),
            decoder_fallbacks: numbers.grouped_i64(snapshot.decoder_fallback_count()),
            present_histogram,
            cross_thread_presents: numbers.grouped_i64(snapshot.cross_thread_present_count),
        }
    }

    /// C# `ReasonText(string reason)`: a translated sentence for an `ExplorerReason` wire text (`code|detail|...`).
    /// An unknown code is `diag.explorerReason.unknown` with the WHOLE input text.
    pub fn reason_text(reason: &str, localizer: &Localizer) -> String {
        let (code, details) = explorer_reason::parse(reason);
        // Detail `i` is `details[i]`, or "" when the wire text has fewer details.
        let detail = |index: usize| details.get(index).map(String::as_str).unwrap_or("");
        match code.as_str() {
            TIMEOUT => localizer.format(keys::DIAG_EXPLORER_REASON_TIMEOUT, &[]),
            CANCELED => localizer.format(keys::DIAG_EXPLORER_REASON_CANCELED, &[]),
            QUERY_FAILED => localizer.format(
                keys::DIAG_EXPLORER_REASON_QUERY_FAILED,
                &[LocArg::new("errorType", detail(0))],
            ),
            SHELL_UNAVAILABLE => localizer.format(keys::DIAG_EXPLORER_REASON_SHELL_UNAVAILABLE, &[]),
            NATIVE_VIEW_FAILED => localizer.format(
                keys::DIAG_EXPLORER_REASON_NATIVE_VIEW_FAILED,
                &[LocArg::new("errorType", detail(0)), LocArg::new("hresult", detail(1))],
            ),
            NO_MATCHING_WINDOW => localizer.format(
                keys::DIAG_EXPLORER_REASON_NO_MATCHING_WINDOW,
                &[LocArg::new("count", detail(0))],
            ),
            COM_CALL_FAILED => localizer.format(
                keys::DIAG_EXPLORER_REASON_COM_CALL_FAILED,
                &[LocArg::new("call", detail(0)), LocArg::new("hresult", detail(1))],
            ),
            EMPTY_VIEW => localizer.format(
                keys::DIAG_EXPLORER_REASON_EMPTY_VIEW,
                &[LocArg::new("hresult", detail(0)), LocArg::new("count", detail(1))],
            ),
            EMPTY_PATH => localizer.format(keys::DIAG_EXPLORER_REASON_EMPTY_PATH, &[]),
            OUTSIDE_FOLDER => localizer.format(keys::DIAG_EXPLORER_REASON_OUTSIDE_FOLDER, &[]),
            DUPLICATE_ITEM => localizer.format(keys::DIAG_EXPLORER_REASON_DUPLICATE_ITEM, &[]),
            INCOMPLETE_SNAPSHOT => localizer.format(keys::DIAG_EXPLORER_REASON_INCOMPLETE_SNAPSHOT, &[]),
            _ => localizer.format(
                keys::DIAG_EXPLORER_REASON_UNKNOWN,
                &[LocArg::new("reason", reason.to_string())],
            ),
        }
    }

    /// C# `StatusText(ExplorerOrderStatus)`: one catalog text per variant (`enum.explorerOrderStatus.*`).
    pub fn status_text(status: ExplorerOrderStatus, localizer: &Localizer) -> String {
        let key = match status {
            ExplorerOrderStatus::Available => keys::ENUM_EXPLORER_ORDER_STATUS_AVAILABLE,
            ExplorerOrderStatus::NoMatchingWindow => keys::ENUM_EXPLORER_ORDER_STATUS_NO_MATCHING_WINDOW,
            ExplorerOrderStatus::NativeViewUnavailable => keys::ENUM_EXPLORER_ORDER_STATUS_NATIVE_VIEW_UNAVAILABLE,
            ExplorerOrderStatus::InvalidSnapshot => keys::ENUM_EXPLORER_ORDER_STATUS_INVALID_SNAPSHOT,
            ExplorerOrderStatus::TimedOut => keys::ENUM_EXPLORER_ORDER_STATUS_TIMED_OUT,
            ExplorerOrderStatus::Canceled => keys::ENUM_EXPLORER_ORDER_STATUS_CANCELED,
            ExplorerOrderStatus::Failed => keys::ENUM_EXPLORER_ORDER_STATUS_FAILED,
        };
        localizer.get(key)
    }

    /// The English member name of an `ExplorerGroupState` as C# `ToString()` writes it (`"None"`, `"Active"`,
    /// `"Unknown"`). Used for `diagnostics.explorerMetadata`'s `{groupState}` (the C# enum is passed unformatted).
    pub fn group_state_name(state: ExplorerGroupState) -> &'static str {
        match state {
            ExplorerGroupState::None => "None",
            ExplorerGroupState::Active => "Active",
            ExplorerGroupState::Unknown => "Unknown",
        }
    }
}

/// C# `Path.GetFileName(string)` for the paths the app shows: the text after the last `\` or `/`; a path without a
/// separator is returned as it is; a path ending in a separator gives `""`. Never a `Path` method (see contract trap T5).
pub fn file_name(path: &str) -> &str {
    match path.rfind(['\\', '/']) {
        // The separators are ASCII, so `index + 1` is a char boundary.
        Some(index) => &path[index + 1..],
        None => path,
    }
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
        items
            .iter()
            .map(|item| Self::build_item(item, localizer, numbers))
            .collect()
    }

    /// One list line: `batchReview.item` when the size is known, `batchReview.itemMissing` otherwise (C# `BuildItem`).
    pub fn build_item(item: &BatchReviewItem, localizer: &Localizer, numbers: &dyn NumberFormatting) -> String {
        let path = item.path.to_string_lossy().into_owned();
        let file = file_name(&path).to_string();
        match item.length {
            // T12: `Some(0)` is a known size; only `None` is the missing line.
            Some(length) => localizer.format(
                keys::BATCH_REVIEW_ITEM,
                &[
                    LocArg::new("fileName", file),
                    LocArg::new("size", numbers.grouped_i64(i64::try_from(length).unwrap_or(i64::MAX))),
                    LocArg::new("path", path),
                ],
            ),
            None => localizer.format(
                keys::BATCH_REVIEW_ITEM_MISSING,
                &[LocArg::new("fileName", file), LocArg::new("path", path)],
            ),
        }
    }

    /// The heading under the title, `batchReview.summary` (plural by `count`, C# `Summary(int)`).
    pub fn summary(count: i64, localizer: &Localizer) -> String {
        localizer.format_plural(BATCH_REVIEW_SUMMARY, count, &[LocArg::new("count", count.to_string())])
    }

    /// The path-only form (sizes unknown): every item has `length: None` (C# `FromPaths`).
    pub fn from_paths(paths: &[PathBuf]) -> Vec<BatchReviewItem> {
        paths
            .iter()
            .map(|path| BatchReviewItem {
                path: path.clone(),
                length: None,
            })
            .collect()
    }
}

/// C# `SkippedFilesPresenter` (static class).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkippedFilesPresenter;

impl SkippedFilesPresenter {
    /// `main.skipped.window.title` (C# `Title`).
    pub fn title(localizer: &Localizer) -> String {
        localizer.get(keys::MAIN_SKIPPED_WINDOW_TITLE)
    }

    /// `main.skipped.window.intro` (C# `Intro`).
    pub fn intro(localizer: &Localizer) -> String {
        localizer.get(keys::MAIN_SKIPPED_WINDOW_INTRO)
    }

    /// `main.skipped.window.close` (C# `CloseText`).
    pub fn close_text(localizer: &Localizer) -> String {
        localizer.get(keys::MAIN_SKIPPED_WINDOW_CLOSE)
    }

    /// One line per entry, in order (C# `BuildLines`).
    pub fn build_lines(entries: &[SkippedEntry], localizer: &Localizer) -> Vec<String> {
        entries
            .iter()
            .map(|entry| Self::format_entry(entry, localizer))
            .collect()
    }

    /// `skipped.entry` with the full path and the reason sentence (C# `FormatEntry`). The OS message stays verbatim
    /// inside `skipped.reason.fileUnreadable` or `skipped.reason.listingInterrupted`.
    pub fn format_entry(entry: &SkippedEntry, localizer: &Localizer) -> String {
        let reason_key = match entry.kind {
            SkippedKind::FileUnreadable => keys::SKIPPED_REASON_FILE_UNREADABLE,
            SkippedKind::ListingInterrupted => keys::SKIPPED_REASON_LISTING_INTERRUPTED,
        };
        let reason = localizer.format(reason_key, &[LocArg::new("detail", entry.reason.clone())]);
        localizer.format(
            keys::SKIPPED_ENTRY,
            &[
                LocArg::new("path", entry.path.to_string_lossy().into_owned()),
                LocArg::new("reason", reason),
            ],
        )
    }
}
