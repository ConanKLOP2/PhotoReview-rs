//! Port of `BenchmarkDialog.cs` lines 44 to 103 (fields, constructor, properties, the `NativeDialog` overrides), 138 to 330 (`OnInitialized`, `IndexOfProfile`,
//! `ColumnHeaders`, `AddColumns`, `OnClosed`, `OnLanguageChanged`, `OnRowsChanged`, `SyncRows`, `CompareRows`, `SortBy`), baseline `d92d6aae`. Unit W8-18r.
//! Contract: `contracts/W8-18r.md`.
//!
//! `BenchmarkContent` is the C# class without its base: the one `impl DialogContent` of the window (D-W8B5-301) whose hooks call inherent methods that `layout.rs`
//! (18r), `results.rs` and `run.rs` (18s) add. `BenchmarkDialog` is `{ native, content }` (the handle plus the content).
//!
//! Threading and borrows (D-W8B5-504): everything here runs on the UI thread. The presenter sits in `Rc<RefCell<..>>` and is borrowed for ONE presenter call at a
//! time; the `BenchmarkView` the presenter writes to (`run::DialogView`) never reads the presenter back (the grid is rebuilt after the call, from `rows_dirty`).
use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use pr_app::coordinators::zoom_detail::BackgroundRunner;
use pr_app::presenters::benchmark::{
    BenchmarkFileScan, BenchmarkPresenter, BenchmarkSessionFactory, BenchmarkSummarySave,
};
use pr_app::presenters::benchmark_presentation::BenchmarkResultRow;
use pr_app::presenters::settings_field_map::FieldContext;
use pr_l10n::localizer::{CurrentChangedSubscription, Localizer};

use super::layout::{COLUMN_COUNT, LayoutState};
use super::run::RunState;
use crate::dialogs::native::chrome::NativeDialogChrome;
use crate::dialogs::native::dialog::{DialogContent, ListItemText, NativeDialog};
use crate::dialogs::native::template::DialogItem;
use crate::win32::messages::WM_APP;
use crate::win32::structs::{Hwnd, Lparam, Lresult, Wparam};

/// Private message posted by a worker thread to the dialog window: "the inbox has events" (D-W8B5-505). `wparam` and `lparam` are 0.
pub const WM_BENCHMARK_WAKE: u32 = WM_APP + 0x42;
/// Private message posted by the language-change handler (any thread): "retranslate the window" (C# `_ui.Post(OnLanguageChanged)`).
pub const WM_BENCHMARK_LANGUAGE: u32 = WM_APP + 0x43;

/// C# `Func<nint, string, string?, string?> pickFolder`: `(owner, title, initial_folder)` to the chosen folder or `None`.
pub type PickFolderFn = Rc<dyn Fn(Hwnd, &str, Option<&str>) -> Option<String>>;
/// C# `Action<string> openInExplorer`: starts Explorer with these arguments.
pub type OpenInExplorerFn = Rc<dyn Fn(&str)>;

/// The optional constructor arguments of the C# `BenchmarkDialog` (`pickFolder`, `openInExplorer`, `scan`, `save`, `chrome`; `ui` has no Rust twin, D-W8B5-505) plus
/// the worker seam and the Pictures-folder override of the tests. `Default` = every field `None` = the C# `null` defaults.
#[derive(Default)]
pub struct BenchmarkOptions {
    /// `None` = a function that always answers `None` (C# `(_, _, _) => null`).
    pub pick_folder: Option<PickFolderFn>,
    /// `None` = [`default_open_in_explorer`].
    pub open_in_explorer: Option<OpenInExplorerFn>,
    /// `None` = the presenter default (`enumerate_image_files`).
    pub scan: Option<BenchmarkFileScan>,
    /// `None` = the presenter default (report store under the temp folder).
    pub save: Option<BenchmarkSummarySave>,
    /// `None` = `default_chrome()`.
    pub chrome: Option<Rc<dyn NativeDialogChrome>>,
    /// Where scan, profile and save jobs run. `None` = `ThreadRunner` (one named thread per job). Tests pass an inline or queued runner.
    pub runner: Option<Arc<dyn BackgroundRunner>>,
    /// The folder shown when `initial_folder` is `None`. `None` = [`pictures_folder`].
    pub pictures_folder: Option<String>,
}

impl fmt::Debug for BenchmarkOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BenchmarkOptions").finish_non_exhaustive()
    }
}

/// C# `Environment.GetFolderPath(MyPictures)`: `SHGetKnownFolderPath(FOLDERID_Pictures)` (one SAFETY-commented call; the returned buffer is freed with
/// `CoTaskMemFree` on every path); `""` when the shell cannot answer (what .NET returns).
pub fn pictures_folder() -> String {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Pictures, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    // SAFETY: SHGetKnownFolderPath returns an allocated NUL-terminated path that is freed below.
    let Ok(raw) = (unsafe { SHGetKnownFolderPath(&FOLDERID_Pictures, KF_FLAG_DEFAULT, None) }) else {
        return String::new();
    };
    if raw.is_null() {
        return String::new();
    }
    // SAFETY: A successful known-folder call returns a valid NUL-terminated UTF-16 string.
    let path = unsafe { raw.to_string().unwrap_or_default() };
    // SAFETY: `raw` was allocated by SHGetKnownFolderPath and must be freed with CoTaskMemFree.
    unsafe {
        CoTaskMemFree(Some(raw.0.cast()));
    }
    path
}

/// C# `StartExplorer(arguments)`, the default of `openInExplorer`: `SettingsPresenter::try_start_explorer(arguments)`; `Some(message)` is logged as
/// `AppLog::global().error("Could not start Explorer", ...)` and swallowed (C# caught `Win32Exception` and `InvalidOperationException`).
pub fn default_open_in_explorer(arguments: &str) {
    if let Some(message) = pr_app::presenters::settings::SettingsPresenter::try_start_explorer(arguments) {
        let error = std::io::Error::other(message);
        pr_app::misc::app_log::AppLog::global().error("Could not start Explorer", Some(&error));
    }
}

/// C# `ColumnHeaders()`: `[BEST_SYMBOL, benchmark.column.profile, benchmark.column.loadingMode, benchmark.column.workload, benchmark.column.samples, "P50", "P95",
/// "P99", "Max", benchmark.column.status]` (`tr::benchmark_column_*`, the localizer given).
pub fn column_headers(texts: &Localizer) -> [String; COLUMN_COUNT] {
    [
        pr_app::presenters::benchmark_presentation::BEST_SYMBOL.to_owned(),
        texts.get("benchmark.column.profile"),
        texts.get("benchmark.column.loadingMode"),
        texts.get("benchmark.column.workload"),
        texts.get("benchmark.column.samples"),
        "P50".into(),
        "P95".into(),
        "P99".into(),
        "Max".into(),
        texts.get("benchmark.column.status"),
    ]
}

/// .NET `CultureInfo.CurrentCulture.CompareInfo.Compare(a, b, CompareOptions.None)` for the text columns: `CompareStringEx(LOCALE_NAME_USER_DEFAULT, 0, a, b)`
/// (`CSTR_LESS_THAN` / `CSTR_EQUAL` / `CSTR_GREATER_THAN`); a failure (`0`) falls back to `pr_core::text::cmp_ignore_case`, then to the ordinal order (D-W8B5-509).
pub fn culture_compare(a: &str, b: &str) -> Ordering {
    use windows::Win32::Globalization::{
        COMPARE_STRING_FLAGS, CSTR_EQUAL, CSTR_GREATER_THAN, CSTR_LESS_THAN, CompareStringEx,
    };
    use windows::core::PCWSTR;
    if a.is_empty() || b.is_empty() {
        return a.len().cmp(&b.len());
    }
    let left: Vec<u16> = a.encode_utf16().collect();
    let right: Vec<u16> = b.encode_utf16().collect();
    // SAFETY: both slices hold live UTF-16 data for the synchronous comparison; null locale selects the user's locale, and optional pointers are absent.
    match unsafe { CompareStringEx(PCWSTR::null(), COMPARE_STRING_FLAGS(0), &left, &right, None, None, None) } {
        CSTR_LESS_THAN => Ordering::Less,
        CSTR_EQUAL => Ordering::Equal,
        CSTR_GREATER_THAN => Ordering::Greater,
        _ => pr_core::text::cmp_ignore_case(a, b).then_with(|| a.cmp(b)),
    }
}

/// .NET `double.CompareTo`: NaN is less than every number and equal to NaN; `-0.0` equals `0.0`.
pub fn compare_f64_dotnet(a: f64, b: f64) -> Ordering {
    if a.is_nan() {
        if b.is_nan() { Ordering::Equal } else { Ordering::Less }
    } else if b.is_nan() {
        Ordering::Greater
    } else {
        a.partial_cmp(&b).unwrap_or(Ordering::Equal)
    }
}

/// C# `CompareRows(left, right, column)` (internal static): column 4 compares `count()` (`i32`); 5, 6, 7, 8 compare `phase.p50()`, `p95()`, `p99()`, `max()` with
/// [`compare_f64_dotnet`]; every other column (0, 1, 2, 3, 9 and any out-of-range number) compares `cell_texts(texts)[clamp(column, 0, 9)]` with `text_cmp`
/// (written `column.max(0).min(9)`).
pub fn compare_rows(
    left: &BenchmarkResultRow,
    right: &BenchmarkResultRow,
    column: i32,
    texts: &Localizer,
    text_cmp: &dyn Fn(&str, &str) -> Ordering,
) -> Ordering {
    match column {
        4 => left.count().cmp(&right.count()),
        5 => compare_f64_dotnet(left.phase.p50(), right.phase.p50()),
        6 => compare_f64_dotnet(left.phase.p95(), right.phase.p95()),
        7 => compare_f64_dotnet(left.phase.p99(), right.phase.p99()),
        8 => compare_f64_dotnet(left.phase.max(), right.phase.max()),
        _ => {
            let l = left.cell_texts(texts);
            let r = right.cell_texts(texts);
            let i = column.clamp(0, (COLUMN_COUNT - 1) as i32) as usize;
            text_cmp(
                l.get(i).map(String::as_str).unwrap_or(""),
                r.get(i).map(String::as_str).unwrap_or(""),
            )
        }
    }
}

/// The row order of `SyncRows`, pure: the identity `0..count` when `column < 0` or `direction == 0`; otherwise the indices sorted by `compare(a, b)`, ties broken by
/// the index (`a.cmp(&b)`), ascending when `direction == 1`, otherwise (2) descending = the whole result reversed INCLUDING the tie-break (C# `-compare`).
pub fn display_order(
    count: usize,
    column: i32,
    direction: i32,
    compare: &dyn Fn(usize, usize) -> Ordering,
) -> Vec<usize> {
    if column < 0 || direction == 0 {
        return (0..count).collect();
    }
    let mut order: Vec<_> = (0..count).collect();
    order.sort_by(|a, b| compare(*a, *b).then_with(|| a.cmp(b)));
    if direction != 1 {
        order.reverse();
    }
    order
}

/// C# `SortBy` state change, pure: `direction = if current_column == column && current_direction == 1 { 2 } else { 1 }`; returns `(column, direction)`. (Clicking a
/// column that is already descending, or another column, sorts ascending.)
pub fn next_sort(current: (i32, i32), column: i32) -> (i32, i32) {
    (column, if current.0 == column && current.1 == 1 { 2 } else { 1 })
}

/// Builds the `FieldContext` of the ambient language (`Localizer::current()`, `pr_l10n::localizer::number_culture()`) and runs `f` with it (D-W8-4); the `Arc`s live for
/// the duration of the call.
pub fn with_field_context<R>(f: impl FnOnce(&FieldContext<'_>) -> R) -> R {
    let localizer = Localizer::current();
    let culture = pr_l10n::localizer::number_culture();
    let ctx = FieldContext {
        texts: &localizer,
        numbers: &culture,
    };
    f(&ctx)
}

/// The `DialogContent` of the benchmark window (C# `BenchmarkDialog` without its base class).
pub struct BenchmarkContent {
    /// C# `_presenter`, shared with the tooltip provider of the profile list.
    pub(super) presenter: Rc<RefCell<BenchmarkPresenter>>,
    /// C# `Items`: built once in the constructor.
    pub(super) items: Vec<DialogItem>,
    /// C# `_initialFolder`.
    pub(super) initial_folder: String,
    /// C# `_pickFolder`.
    // The folder picker is consumed by the sibling run unit when its command handlers are implemented.
    #[allow(dead_code)]
    pub(super) pick_folder: PickFolderFn,
    /// C# `_openInExplorer`.
    // The explorer callback is consumed by the sibling run unit when its presenter view is implemented.
    #[allow(dead_code)]
    pub(super) open_in_explorer: OpenInExplorerFn,
    /// C# `_textsApplied`: `OnInitialized` ran to its end.
    pub(super) texts_applied: Cell<bool>,
    /// C# `_sortColumn` / `_sortDirection`: `(-1, 0)` = the presenter's order.
    pub(super) sort: Cell<(i32, i32)>,
    /// C# `_languageHandler` subscription: dropping it unsubscribes.
    pub(super) language: RefCell<Option<CurrentChangedSubscription>>,
    /// C# `_statusHeight` (`layout.rs`).
    pub(super) layout: LayoutState,
    /// Inbox, runner, window handle for the wake-ups and the "grid is stale" flag (`run.rs`).
    pub(super) run: RunState,
}

impl fmt::Debug for BenchmarkContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BenchmarkContent")
            .field("sort", &self.sort.get())
            .finish_non_exhaustive()
    }
}

impl BenchmarkContent {
    /// C# constructor body: `presenter = BenchmarkPresenter::new(session_factory, scan, save)` in an `Rc<RefCell<..>>`; `initial_folder =
    /// BenchmarkPresenter::initial_folder(initial_folder, &pictures)` with `pictures = options.pictures_folder` or [`pictures_folder`]; `pick_folder` and
    /// `open_in_explorer` defaulted as documented on [`BenchmarkOptions`]; `items = layout::build_items()` now; `sort = (-1, 0)`; `run = RunState::new(options.runner)`.
    pub fn new(
        initial_folder: Option<&str>,
        session_factory: BenchmarkSessionFactory,
        options: &mut BenchmarkOptions,
    ) -> Self {
        let pictures = options.pictures_folder.take().unwrap_or_else(pictures_folder);
        let initial_folder = BenchmarkPresenter::initial_folder(initial_folder, &pictures);
        let pick_folder = options.pick_folder.take().unwrap_or_else(|| Rc::new(|_, _, _| None));
        let open_in_explorer = options
            .open_in_explorer
            .take()
            .unwrap_or_else(|| Rc::new(default_open_in_explorer));
        let presenter = BenchmarkPresenter::new(session_factory, options.scan.take(), options.save.take());
        Self {
            presenter: Rc::new(RefCell::new(presenter)),
            items: super::layout::build_items(),
            initial_folder,
            pick_folder,
            open_in_explorer,
            texts_applied: Cell::new(false),
            sort: Cell::new((-1, 0)),
            language: RefCell::new(None),
            layout: LayoutState::default(),
            run: RunState::new(options.runner.take()),
        }
    }

    /// C# `IndexOfProfile(item)`: the position of the presenter's default-selection item in `profile_items()` (identity in C#; here the profile id), `-1` when absent.
    fn index_of_default_profile(&self) -> i32 {
        let presenter = self.presenter.borrow();
        let id = &presenter.default_selection().profile.id;
        presenter
            .profile_items()
            .iter()
            .position(|item| &item.profile.id == id)
            .and_then(|i| i32::try_from(i).ok())
            .unwrap_or(-1)
    }

    /// C# `AddColumns()`: for `i` in `0..10`, `ListViewControl::add_column(results, i, column_headers(texts)[i], dialog.dx(40), right_aligned = 4 <= i <= 8)`.
    fn add_columns(&self, dialog: &NativeDialog) {
        use crate::dialogs::native::controls::list_tree::ListViewControl;
        for (i, header) in column_headers(&Localizer::current()).iter().enumerate() {
            ListViewControl::add_column(
                dialog.item(super::layout::RESULTS_ID),
                i as i32,
                header,
                dialog.dx(40),
                (4..=8).contains(&i),
            );
        }
    }

    /// C# `SyncRows()`: nothing when the grid window is `0`. Remember the selected row's source index (`ListViewControl::selected_row` then `row_param`; `-1` when
    /// none); `order = display_order(rows.len(), sort.0, sort.1, ..)` using `compare_rows` + [`culture_compare`] on the presenter's rows (a short `borrow()`, the cells
    /// copied out before any OS call); `ListViewControl::clear`; for each displayed row `add_row(results, cells, source as isize)`; re-select the row whose source equals the
    /// remembered one (`select_row`).
    pub(super) fn sync_rows(&self, dialog: &NativeDialog) {
        use crate::dialogs::native::controls::list_tree::ListViewControl;
        let results = dialog.item(super::layout::RESULTS_ID);
        if results == 0 {
            return;
        }
        let selected = ListViewControl::selected_row(results);
        let source_selected = if selected >= 0 {
            ListViewControl::row_param(results, selected)
        } else {
            -1
        };
        let sort = self.sort.get();
        let texts = Localizer::current();
        let rows = self.presenter.borrow().rows().to_vec();
        let order = display_order(rows.len(), sort.0, sort.1, &|a, b| {
            compare_rows(&rows[a], &rows[b], sort.0, &texts, &culture_compare)
        });
        ListViewControl::clear(results);
        let mut selected_row = -1;
        for (index, source) in order.iter().copied().enumerate() {
            ListViewControl::add_row(results, &rows[source].cell_texts(&texts), source as isize);
            if source as isize == source_selected {
                selected_row = index as i32;
            }
        }
        if selected_row >= 0 {
            ListViewControl::select_row(results, selected_row);
        }
    }

    /// Runs `sync_rows` when `run.rows_dirty` is set (and clears it). Called after every presenter call that was given a `DialogView`.
    pub(super) fn flush_view_effects(&self, dialog: &NativeDialog) {
        if self.run.rows_dirty.replace(false) {
            self.sync_rows(dialog);
        }
    }

    /// C# `SortBy(column)`: `sort = next_sort(sort, column)`; `dialog.set_list_view_sort(RESULTS_ID, column, direction)`; `sync_rows`.
    pub(super) fn sort_by(&self, dialog: &NativeDialog, column: i32) {
        let next = next_sort(self.sort.get(), column);
        self.sort.set(next);
        dialog.set_list_view_sort(super::layout::RESULTS_ID, column, next.1);
        self.sync_rows(dialog);
    }

    /// C# `OnLanguageChanged()`: nothing when the dialog is closed or has no window. `set_window_text(hwnd, tr::benchmark_window_title())`; the texts of header, description,
    /// browse, limit label, profiles label, select all, select none, run, quick check, cancel run (`common_cancel`), open report, open result folder, close; each column header
    /// (`ListViewControl::set_column_text`); `presenter.notify_language_changed(&mut view)` + `flush_view_effects` (this redraws names and cells); `invalidate_rect` of the
    /// profile list; `run_layout`.
    pub(super) fn on_language_changed(&self, dialog: &NativeDialog) {
        if dialog.is_closed() || dialog.hwnd() == 0 {
            return;
        }
        use super::layout::*;
        use crate::dialogs::native::controls::list_tree::ListViewControl;
        let title = pr_l10n::tr::benchmark_window_title();
        let _ = crate::win32::dialog_native::user32::set_window_text(dialog.hwnd(), &title);
        for (id, text) in [
            (HEADER_ID, pr_l10n::tr::benchmark_header()),
            (DESCRIPTION_ID, pr_l10n::tr::benchmark_description()),
            (BROWSE_ID, pr_l10n::tr::benchmark_browse()),
            (LIMIT_LABEL_ID, pr_l10n::tr::benchmark_image_limit_label()),
            (PROFILES_LABEL_ID, pr_l10n::tr::benchmark_profiles_label()),
            (SELECT_ALL_ID, pr_l10n::tr::benchmark_select_all()),
            (SELECT_NONE_ID, pr_l10n::tr::benchmark_select_none()),
            (RUN_ID, pr_l10n::tr::benchmark_run()),
            (QUICK_CHECK_ID, pr_l10n::tr::benchmark_quick_check()),
            (CANCEL_RUN_ID, pr_l10n::tr::common_cancel()),
            (OPEN_REPORT_ID, pr_l10n::tr::benchmark_open_report()),
            (OPEN_FOLDER_ID, pr_l10n::tr::benchmark_open_result_folder()),
            (CLOSE_ID, pr_l10n::tr::common_close()),
        ] {
            dialog.set_item_text(id, &text);
        }
        for (i, text) in column_headers(&Localizer::current()).iter().enumerate() {
            ListViewControl::set_column_text(dialog.item(RESULTS_ID), i as i32, text);
        }
        if let Ok(presenter) = self.presenter.try_borrow_mut() {
            let mut view = super::run::DialogView { dialog, content: self };
            presenter.notify_language_changed(&mut view);
        }
        self.flush_view_effects(dialog);
        let _ = crate::win32::dialog_native::user32::invalidate_rect(dialog.item(PROFILES_ID), None, true);
        dialog.run_layout();
    }
}

impl DialogContent for BenchmarkContent {
    /// `tr::benchmark_window_title()`.
    fn title(&self) -> String {
        pr_l10n::tr::benchmark_window_title()
    }

    /// The list built in `new` (cloned).
    fn items(&self) -> Vec<DialogItem> {
        self.items.clone()
    }

    /// C# `Resizable` = `true`.
    fn resizable(&self) -> bool {
        true
    }

    /// C# `UsesModalFrame` = `false` (a normal window with its own caption buttons).
    fn uses_modal_frame(&self) -> bool {
        false
    }

    /// C# `ExtraWindowStyle` = `WS_MINIMIZEBOX | WS_MAXIMIZEBOX`.
    fn extra_window_style(&self) -> u32 {
        crate::win32::messages::WS_MINIMIZEBOX | crate::win32::messages::WS_MAXIMIZEBOX
    }

    /// C# `HeaderControlIds` = `[layout::HEADER_ID]`.
    fn header_control_ids(&self) -> Vec<i32> {
        vec![super::layout::HEADER_ID]
    }

    /// C# `InitialFocusId` = `layout::RUN_ID`.
    fn initial_focus_id(&self) -> i32 {
        super::layout::RUN_ID
    }

    /// C# `DefaultButtonId` = `layout::RUN_ID`.
    fn default_button_id(&self) -> i32 {
        super::layout::RUN_ID
    }

    /// `layout::preferred_client_size(dialog)`.
    fn preferred_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        super::layout::preferred_client_size(dialog)
    }

    /// `layout::minimum_client_size(dialog)`.
    fn minimum_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        super::layout::minimum_client_size(dialog)
    }

    /// `layout::apply_layout(dialog, &layout::layout_plan(dialog, client_width, client_height), &self.layout)`.
    fn layout_controls(&self, dialog: &NativeDialog, client_width: i32, client_height: i32) {
        super::layout::apply_layout(
            dialog,
            &super::layout::layout_plan(dialog, client_width, client_height),
            &self.layout,
        )
    }

    /// C# `OnInitialized()` (lines 140 to 172), in this order: `run.hwnd.store(dialog.hwnd())` (so workers can wake the window); folder box = `initial_folder`; limit box =
    /// `BenchmarkPresenter::default_image_limit_text()`; every profile name (`item.name(&Localizer::current())`) added to the profile list; the default profile selected
    /// (`ListBoxControl::set_selected`); the grid configured (`ListViewControl::configure(results, from_key("Dark.Window"), DialogColors::text())`, a missing key falls back
    /// to `DialogColors::window()`), `add_columns`; `ProgressControl::configure`; `set_item_tooltips(PROFILES_ID, ..)` with a provider that `try_borrow`s the shared presenter
    /// and answers `profile_items()[index].description(&Localizer::current())` (`None` out of range or while the presenter is borrowed); `set_tooltip` of the folder box
    /// (`tr::benchmark_folder_automation_name()`), the limit box (`benchmark_image_limit_automation_name`) and Quick check (`benchmark_quick_check_automation_name`); subscribe
    /// to language changes (`pr_l10n::localizer::on_current_changed`, the handler posts `WM_BENCHMARK_LANGUAGE` to the window; stored in `language`); `texts_applied = true`.
    fn on_initialized(&self, dialog: &NativeDialog) {
        use super::layout::*;
        use crate::dialogs::native::controls::{
            basic::ListBoxControl, custom::ProgressControl, list_tree::ListViewControl,
        };
        self.run.hwnd.store(dialog.hwnd(), std::sync::atomic::Ordering::Release);
        dialog.set_item_text(FOLDER_ID, &self.initial_folder);
        dialog.set_item_text(LIMIT_ID, &BenchmarkPresenter::default_image_limit_text());
        let list = dialog.item(PROFILES_ID);
        let profiles = self.presenter.borrow().profile_items().to_vec();
        let texts = Localizer::current();
        for item in &profiles {
            ListBoxControl::add(list, &item.name(&texts));
        }
        let default_index = self.index_of_default_profile();
        if default_index >= 0 {
            ListBoxControl::set_selected(list, default_index, true);
        }
        let results = dialog.item(RESULTS_ID);
        ListViewControl::configure(
            results,
            crate::dialogs::native::chrome::DialogColors::from_key("Dark.Window")
                .unwrap_or_else(crate::dialogs::native::chrome::DialogColors::window),
            crate::dialogs::native::chrome::DialogColors::text(),
        );
        self.add_columns(dialog);
        ProgressControl::configure(dialog.item(PROGRESS_ID));
        let presenter = Rc::clone(&self.presenter);
        dialog.set_item_tooltips(
            PROFILES_ID,
            Rc::new(move |index| {
                presenter.try_borrow().ok().and_then(|p| {
                    p.profile_items()
                        .get(index as usize)
                        .map(|x| x.description(&Localizer::current()))
                })
            }),
        );
        dialog.set_tooltip(FOLDER_ID, &pr_l10n::tr::benchmark_folder_automation_name());
        dialog.set_tooltip(LIMIT_ID, &pr_l10n::tr::benchmark_image_limit_automation_name());
        dialog.set_tooltip(QUICK_CHECK_ID, &pr_l10n::tr::benchmark_quick_check_automation_name());
        let hwnd = self.run.hwnd.clone();
        let subscription = pr_l10n::localizer::on_current_changed(move || {
            let handle = hwnd.load(std::sync::atomic::Ordering::Acquire);
            if handle != 0 {
                // SAFETY: This posts a pointer-free private message to the atomically published window handle.
                unsafe {
                    let _ = crate::win32::api::user32::post_message(handle, WM_BENCHMARK_LANGUAGE, 0, 0);
                }
            }
        });
        *self.language.borrow_mut() = Some(subscription);
        self.texts_applied.set(true);
    }

    /// C# `OnClosed()`: drop the language subscription; `run.hwnd.store(0)`; `presenter.try_borrow_mut()` then `close()` (cancels a run in flight; never a Fail row).
    fn on_closed(&self, _dialog: &NativeDialog) {
        self.language.borrow_mut().take();
        self.run.hwnd.store(0, std::sync::atomic::Ordering::Release);
        if let Ok(mut presenter) = self.presenter.try_borrow_mut() {
            presenter.close();
        }
    }

    /// C# `OnCommand(id, notification)`: `self.command(dialog, id, notification)` (`run.rs`).
    fn on_command(&self, dialog: &NativeDialog, id: i32, notification: i32) -> bool {
        self.command(dialog, id, notification)
    }

    /// C# `OnCancel()`: `self.cancel_requested(dialog)` (`run.rs`).
    fn on_cancel(&self, dialog: &NativeDialog) -> bool {
        self.cancel_requested(dialog)
    }

    /// C# `OnNotify(id, code, header, out result)`: `self.notify(dialog, id, code, header)` (`results.rs`).
    fn on_notify(&self, dialog: &NativeDialog, id: i32, code: i32, header: Lparam) -> Option<Lresult> {
        self.notify(dialog, id, code, header)
    }

    /// C# `ListItemText(id, index)`: `self.list_text(dialog, id, index)` (`results.rs`).
    fn list_item_text(&self, dialog: &NativeDialog, id: i32, index: i32) -> ListItemText {
        self.list_text(dialog, id, index)
    }

    /// C# `StaticTextColor(id)`: `self.static_color(dialog, id)` (`results.rs`).
    fn static_text_color(&self, dialog: &NativeDialog, id: i32) -> u32 {
        self.static_color(dialog, id)
    }

    /// `WM_BENCHMARK_WAKE` -> `self.drain_inbox(dialog)` (`run.rs`) and `Some(0)`; `WM_BENCHMARK_LANGUAGE` -> `on_language_changed` and `Some(0)`; anything else `None`.
    fn on_window_message(
        &self,
        dialog: &NativeDialog,
        message: u32,
        _wparam: Wparam,
        _lparam: Lparam,
    ) -> Option<Lresult> {
        match message {
            WM_BENCHMARK_WAKE => {
                self.drain_inbox(dialog);
                Some(0)
            }
            WM_BENCHMARK_LANGUAGE => {
                self.on_language_changed(dialog);
                Some(0)
            }
            _ => None,
        }
    }
}

/// The benchmark window (C# `BenchmarkDialog`). Show it with `dialog.native().show_modeless(owner, loop, ready, closed)`; one window per shell (the router, W8-18o,
/// keeps the single instance and calls `native().activate()` for a second open).
pub struct BenchmarkDialog {
    native: NativeDialog,
    content: Rc<BenchmarkContent>,
}

impl fmt::Debug for BenchmarkDialog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BenchmarkDialog")
            .field("native", &self.native)
            .finish_non_exhaustive()
    }
}

impl BenchmarkDialog {
    /// C# `BenchmarkDialog(initialFolder, ui, sessionFactory, pickFolder, openInExplorer, scan, save, chrome)`: `ui` is dropped (D-W8B5-505), the optional arguments are
    /// [`BenchmarkOptions`]. `content = Rc::new(BenchmarkContent::new(..))`, `native = NativeDialog::with_chrome(content.clone(), options.chrome.take())`.
    pub fn new(
        initial_folder: Option<&str>,
        session_factory: BenchmarkSessionFactory,
        options: BenchmarkOptions,
    ) -> Self {
        let mut options = options;
        let chrome = options.chrome.take();
        let content = Rc::new(BenchmarkContent::new(initial_folder, session_factory, &mut options));
        let native = NativeDialog::with_chrome(content.clone(), chrome);
        Self { native, content }
    }

    /// The base-class handle (`show_modeless`, `activate`, `close`, `hwnd`, `is_closed`, `item`, ...).
    pub fn native(&self) -> &NativeDialog {
        &self.native
    }

    /// C# `Presenter` (test seam): the shared presenter (`profile_items`, `rows`, `is_running`, `last_report_path`).
    pub fn presenter(&self) -> Rc<RefCell<BenchmarkPresenter>> {
        Rc::clone(&self.content.presenter)
    }

    /// C# `Sort` (test seam): `(column, direction)`, `(-1, 0)` = the presenter's order, direction 1 ascending and 2 descending.
    pub fn sort(&self) -> (i32, i32) {
        self.content.sort.get()
    }

    /// C# `SortBy(column)` (test seam and the header click).
    pub fn sort_by(&self, column: i32) {
        self.content.sort_by(&self.native, column);
    }

    /// C# `ProfileBadge(index)` (test seam): the badge (loading mode text) of profile `index`, `None` out of range. Implemented as `results::profile_list_text(items,
    /// index, &Localizer::current())` (18s) `.and_then(|t| t.badge)`; the presenter is read with `try_borrow` (`None` while borrowed).
    pub fn profile_badge(&self, index: i32) -> Option<String> {
        let profiles = self.content.presenter.try_borrow().ok()?.profile_items().to_vec();
        super::results::profile_list_text(&profiles, index, &Localizer::current()).and_then(|text| text.badge)
    }

    /// C# `Dispose()`: `presenter.close()` (cancels the run in flight and frees the presenter's token). Safe to call twice.
    pub fn dispose(&self) {
        if let Ok(mut presenter) = self.content.presenter.try_borrow_mut() {
            presenter.close();
        }
    }
}
