//! Port of `PhotoReview.App.Shared/ViewModels/CompareViewModel.cs` (289 lines, baseline `d92d6aae`): the state of the
//! side-by-side compare view (two paths, two preview images, the selected side, size texts, hash text, status line) and the
//! staged load that fills it. Contract: `contracts/W8-06c.md`.
//!
//! Decisions (see the contract):
//! - D-W8-1: no `INotifyPropertyChanged`. Every mutator returns a [`CompareChanges`] bit set; the shell repaints from it.
//! - D-W8-2: no `async`. C# `LoadAsync` awaits three times (both previews, both file sizes, both hashes). The Rust form is a
//!   staged state machine: [`CompareViewModel::begin_load`] runs the C# code up to the first `await` and returns
//!   [`CompareNext::Await`] with a [`CompareLoad`] that names the blocking [`CompareWork`] to run on worker threads; the
//!   caller feeds the outcome to [`CompareViewModel::resume_load`] on the UI thread. The token (INV-1) is checked at the same
//!   points as in C#.
//! - D-W8-4: texts come from `crate::viewmodels::format::status` and `pr_l10n` (`compare.sizePair`); no literals here.
//! - The preview payload is `Arc<DecodedImage>` (the type of `IPresentationSink::set_current_image`, D-06d-6). C# `SetProperty`
//!   compares the `object?` by reference equality, so the Rust setters compare with `Arc::ptr_eq`.
//!
//! Pure module: no `unsafe`, no `windows` crate.

// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]

use std::fmt;
use std::sync::Arc;

use pr_core::abstractions::{FileSystem, Log};
use pr_core::sync::CancelToken;
use pr_imaging::decode_types::DecodedImage;

use crate::misc::file_hash::FileHasher;
use crate::viewmodels::format::context::FormatContext;

/// The properties C# raises `PropertyChanged` for, in the order `CompareViewModel.Clear()` raises them (which is also the
/// declaration order). The derived `Ord` is this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum CompareProperty {
    IsVisible = 0,
    LeftPath = 1,
    RightPath = 2,
    LeftImage = 3,
    RightImage = 4,
    SelectedPath = 5,
    /// Derived: `LeftPath` non-empty and equal to `SelectedPath` (ordinal, ignoring case).
    IsLeftSelected = 6,
    /// Derived: the right twin of `IsLeftSelected`.
    IsRightSelected = 7,
    LeftSizeText = 8,
    RightSizeText = 9,
    /// Derived: `compare.sizePair` of the two size texts.
    SizeText = 10,
    HashText = 11,
    StatusText = 12,
}

impl CompareProperty {
    /// Every variant, in declaration order (C# `Clear()` order).
    pub const ALL: [CompareProperty; 13] = [
        CompareProperty::IsVisible,
        CompareProperty::LeftPath,
        CompareProperty::RightPath,
        CompareProperty::LeftImage,
        CompareProperty::RightImage,
        CompareProperty::SelectedPath,
        CompareProperty::IsLeftSelected,
        CompareProperty::IsRightSelected,
        CompareProperty::LeftSizeText,
        CompareProperty::RightSizeText,
        CompareProperty::SizeText,
        CompareProperty::HashText,
        CompareProperty::StatusText,
    ];

    /// The C# property name (`nameof(CompareViewModel.X)`), e.g. `"IsLeftSelected"`.
    pub fn csharp_name(self) -> &'static str {
        todo!()
    }
}

/// The set of properties a mutating call changed (D-W8-1). `Default` is the empty set. A property is in the set when C#
/// would have raised `PropertyChanged` for it during that call; the set loses the raise ORDER and repeat count, which no
/// C# test and no consumer reads (the tests use `Assert.Contains`). `iter` yields in [`CompareProperty`] order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompareChanges(u16);

impl CompareChanges {
    /// The empty set.
    pub const EMPTY: CompareChanges = CompareChanges(0);

    /// The set holding every property (what `clear` returns).
    pub fn all() -> Self {
        todo!()
    }

    /// Adds one property.
    pub fn insert(&mut self, property: CompareProperty) {
        todo!()
    }

    /// True when `property` is in the set.
    pub fn contains(self, property: CompareProperty) -> bool {
        todo!()
    }

    /// True when nothing changed.
    pub fn is_empty(self) -> bool {
        todo!()
    }

    /// Set union.
    pub fn union(self, other: CompareChanges) -> CompareChanges {
        todo!()
    }

    /// The members in [`CompareProperty`] order.
    pub fn iter(self) -> std::vec::IntoIter<CompareProperty> {
        todo!()
    }
}

/// Why a preview load or a hash was not produced. Replaces the C# exception that escapes `LoadAsync`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompareError {
    /// C# `OperationCanceledException`. Propagates from a preview loader or from a hash.
    Cancelled,
    /// Any other preview failure (C# `IOException` from a decode, ...). Text for the log only.
    Failed(String),
}

impl fmt::Display for CompareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl std::error::Error for CompareError {}

/// C# `Func<string, long?>? getFileSize`: sizes injected by the caller, read on the UI thread, never with I/O here.
pub type FileSizeFn = Arc<dyn Fn(&str) -> Option<i64> + Send + Sync>;

/// The optional arguments of C# `LoadAsync` (`getHashAsync` is reduced to a flag: the caller owns the hashing, see
/// [`CompareWork::ComputeHashes`]).
#[derive(Clone, Default)]
pub struct CompareLoadOptions {
    /// C# `compareSizeEnabled`.
    pub compare_size_enabled: bool,
    /// C# `compareHashEnabled`.
    pub compare_hash_enabled: bool,
    /// C# `getHashAsync != null`: the caller can hash files. With `compare_hash_enabled` but no source, the hash stays "off".
    pub hash_source_available: bool,
    /// C# `getFileSize`: when `Some`, both sizes come from it (synchronously). When `None` and `compare_size_enabled`, the
    /// load asks for [`CompareWork::StatSizes`].
    pub get_file_size: Option<FileSizeFn>,
    /// C# `currentIndex` (zero-based; the formatter shows it 1-based).
    pub current_index: i32,
    /// C# `totalFiles`.
    pub total_files: i32,
    /// C# `initialSelectedPath`: `None` selects the left path.
    pub initial_selected_path: Option<String>,
}

impl fmt::Debug for CompareLoadOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

/// Blocking work the caller runs off the UI thread, then reports through [`CompareWorkResult`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompareWork {
    /// C# `loadImageAsync(left)` and `loadImageAsync(right)`, started BOTH before either is awaited: run them concurrently.
    LoadImages { left: String, right: String },
    /// C# `Task.Run(() => (TryGetFileSize(left), TryGetFileSize(right)))`: use [`try_get_file_size`] for each path.
    StatSizes { left: String, right: String },
    /// C# `TryGetHashAsync(left)` and `TryGetHashAsync(right)`, started both before either is awaited: run concurrently,
    /// each through [`hash_outcome`].
    ComputeHashes { left: String, right: String },
}

/// What a hash attempt gave (C# `TryGetHashAsync` result, plus the cancellation it lets through).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HashOutcome {
    /// The hash text (upper-case hex from `FileHasher::get`).
    Hash(String),
    /// Any failure but cancellation: logged by [`hash_outcome`], the compare shows "hash unknown" and keeps the previews.
    Failed,
    /// The hash call was cancelled: the whole load ends as [`CompareOutcome::Cancelled`].
    Cancelled,
}

/// The outcome the caller reports for the [`CompareWork`] that was asked.
#[derive(Debug, Clone)]
pub enum CompareWorkResult {
    /// Answer to [`CompareWork::LoadImages`]. `Ok(None)` is a loader that returned `null` (no preview).
    Images {
        left: Result<Option<Arc<DecodedImage>>, CompareError>,
        right: Result<Option<Arc<DecodedImage>>, CompareError>,
    },
    /// Answer to [`CompareWork::StatSizes`]: lengths in bytes, `None` when unknown.
    Sizes { left: Option<i64>, right: Option<i64> },
    /// Answer to [`CompareWork::ComputeHashes`].
    Hashes { left: HashOutcome, right: HashOutcome },
}

/// Opaque state of one load that waits for a worker result (the continuation; like `viewport::fit::FitContinuation`).
/// Each load carries its own copy, so a superseded load still finishes its pending step and then returns
/// [`CompareOutcome::Superseded`], exactly like the C# task.
#[derive(Debug, Clone)]
pub struct CompareLoad {
    pair: (String, String),
    token: i64,
    options: CompareLoadOptions,
    stage: CompareStage,
    /// C# local `leftSize` / `rightSize` (the size SUFFIX fragments, kept for the status line).
    left_size: String,
    right_size: String,
}

/// Which `await` of C# `LoadAsync` the load is parked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompareStage {
    Images,
    Sizes,
    Hashes,
}

impl CompareLoad {
    /// The work the caller must run next.
    pub fn work(&self) -> CompareWork {
        todo!()
    }

    /// The token this load was started with.
    pub fn token(&self) -> i64 {
        todo!()
    }
}

/// How a load ended (the C# return value, or the exception that escaped).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompareOutcome {
    /// C# returned `true`: the compare is fully shown.
    Completed,
    /// C# returned `false`: `is_token_current(token)` was false at a check (INV-1). Nothing later was applied.
    Superseded,
    /// A preview loader failed (C# exception): the caller clears the comparison (`clear`).
    Failed(CompareError),
    /// A hash was cancelled (C# `OperationCanceledException`).
    Cancelled,
}

/// What the caller does next.
#[derive(Debug, Clone)]
pub enum CompareNext {
    /// Run `load.work()` on worker thread(s), then call [`CompareViewModel::resume_load`] with the result on the UI thread.
    Await(CompareLoad),
    /// The load ended.
    Done(CompareOutcome),
}

/// Result of one step: what changed on the view model, and what to do next.
#[derive(Debug, Clone)]
pub struct CompareLoadStep {
    pub changes: CompareChanges,
    pub next: CompareNext,
}

/// C# `CompareViewModel.TryGetFileSize(path, stat)`: the file length, or `None` when the file is missing, is a directory, or
/// cannot be inspected. `FileSystem::try_get_file_stat`: `Ok(Some)` gives the length, `Ok(None)` gives `None`, `Err` gives
/// `None` after `log.error("Compare file size unavailable path={path}", Some(&err))` (C# swallows `IOException`,
/// `UnauthorizedAccessException`, `NotSupportedException`, `ArgumentException` and logs them; a NUL in the path is the
/// `InvalidInput` case). A length above `i64::MAX` is not representable and gives `None`. Blocking: call it off the UI thread.
pub fn try_get_file_size(path: &str, file_system: &dyn FileSystem, log: &dyn Log) -> Option<i64> {
    todo!()
}

/// C# `TryGetHashAsync(getHashAsync, path)` over the blocking hasher: `FileHasher::get(Path::new(path), cancel)`.
/// `Ok(hash)` gives [`HashOutcome::Hash`]; `FileHashError::Canceled` gives [`HashOutcome::Cancelled`]; every other error is
/// logged as `log.error("Compare hash failed path={path}", Some(&err))` and gives [`HashOutcome::Failed`]. Blocking.
pub fn hash_outcome(hasher: &dyn FileHasher, path: &str, cancel: &CancelToken, log: &dyn Log) -> HashOutcome {
    todo!()
}

/// C# `CompareViewModel` (`sealed partial class : ObservableObject`). One instance per main window, UI thread only.
#[derive(Debug)]
pub struct CompareViewModel {
    is_visible: bool,
    left_path: Option<String>,
    right_path: Option<String>,
    left_image: Option<Arc<DecodedImage>>,
    right_image: Option<Arc<DecodedImage>>,
    selected_path: Option<String>,
    left_size_text: String,
    right_size_text: String,
    hash_text: String,
    status_text: String,
}

impl CompareViewModel {
    /// C# field initialisers: hidden, no paths, no images, empty size and status texts, and
    /// `hash_text = status::compare_hash_text(None, ctx)` (the "hash off" text of the language current NOW).
    pub fn new(ctx: &FormatContext<'_>) -> Self {
        todo!()
    }

    // ---- getters (C# properties) ----

    pub fn is_visible(&self) -> bool {
        todo!()
    }

    pub fn left_path(&self) -> Option<&str> {
        todo!()
    }

    pub fn right_path(&self) -> Option<&str> {
        todo!()
    }

    pub fn left_image(&self) -> Option<&Arc<DecodedImage>> {
        todo!()
    }

    pub fn right_image(&self) -> Option<&Arc<DecodedImage>> {
        todo!()
    }

    pub fn selected_path(&self) -> Option<&str> {
        todo!()
    }

    /// C# `IsLeftSelected`: `LeftPath` is non-empty and equals `SelectedPath` ordinally ignoring case (`pr_core::text::eq_ignore_case`).
    pub fn is_left_selected(&self) -> bool {
        todo!()
    }

    /// C# `IsRightSelected`.
    pub fn is_right_selected(&self) -> bool {
        todo!()
    }

    pub fn left_size_text(&self) -> &str {
        todo!()
    }

    pub fn right_size_text(&self) -> &str {
        todo!()
    }

    /// C# `SizeText => Tr.CompareSizePair(LeftSizeText, RightSizeText)`: key `compare.sizePair`, arguments `left` and `right`
    /// (`ctx.texts.format_with("compare.sizePair", .., ctx.numbers)`). English: `"{left} \u{2194} {right}"`.
    pub fn size_text(&self, ctx: &FormatContext<'_>) -> String {
        todo!()
    }

    pub fn hash_text(&self) -> &str {
        todo!()
    }

    pub fn status_text(&self) -> &str {
        todo!()
    }

    // ---- setters (C# `SetProperty`: an equal value changes nothing and raises nothing) ----

    /// Changes: `IsVisible`.
    pub fn set_visible(&mut self, value: bool) -> CompareChanges {
        todo!()
    }

    /// Changes: `LeftPath` and `IsLeftSelected` (only when the path text really changes; ordinal, case-sensitive equality).
    pub fn set_left_path(&mut self, value: Option<String>) -> CompareChanges {
        todo!()
    }

    /// Changes: `RightPath` and `IsRightSelected`.
    pub fn set_right_path(&mut self, value: Option<String>) -> CompareChanges {
        todo!()
    }

    /// Changes: `LeftImage` (reference equality, `Arc::ptr_eq`; `None == None`).
    pub fn set_left_image(&mut self, value: Option<Arc<DecodedImage>>) -> CompareChanges {
        todo!()
    }

    /// Changes: `RightImage`.
    pub fn set_right_image(&mut self, value: Option<Arc<DecodedImage>>) -> CompareChanges {
        todo!()
    }

    /// Changes: `SelectedPath`, `IsLeftSelected`, `IsRightSelected` (both flags are notified whenever the path text changes,
    /// even if a flag keeps its value).
    pub fn set_selected_path(&mut self, value: Option<String>) -> CompareChanges {
        todo!()
    }

    /// Changes: `LeftSizeText` and `SizeText`.
    pub fn set_left_size_text(&mut self, value: String) -> CompareChanges {
        todo!()
    }

    /// Changes: `RightSizeText` and `SizeText`.
    pub fn set_right_size_text(&mut self, value: String) -> CompareChanges {
        todo!()
    }

    /// Changes: `HashText`.
    pub fn set_hash_text(&mut self, value: String) -> CompareChanges {
        todo!()
    }

    /// Changes: `StatusText`.
    pub fn set_status_text(&mut self, value: String) -> CompareChanges {
        todo!()
    }

    // ---- commands ----

    /// C# `SelectLeft()`: `SelectedPath = LeftPath`.
    pub fn select_left(&mut self) -> CompareChanges {
        todo!()
    }

    /// C# `SelectRight()`: `SelectedPath = RightPath`.
    pub fn select_right(&mut self) -> CompareChanges {
        todo!()
    }

    /// C# `Select(path)`: `SelectedPath = path`.
    pub fn select(&mut self, path: Option<String>) -> CompareChanges {
        todo!()
    }

    /// C# `Toggle()`: `IsVisible = !IsVisible`.
    pub fn toggle(&mut self) -> CompareChanges {
        todo!()
    }

    /// C# `Clear()`: resets every field (`hash_text` to `status::compare_hash_text(None, ctx)`) and notifies ALL thirteen
    /// properties unconditionally, even those already at their default: returns [`CompareChanges::all`].
    pub fn clear(&mut self, ctx: &FormatContext<'_>) -> CompareChanges {
        todo!()
    }

    // ---- staged load (C# `LoadAsync`) ----

    /// C# `LoadAsync` from the start up to the first `await`.
    ///
    /// 1. `is_token_current(token)` false: `Done(Superseded)` with no change (nothing is touched).
    /// 2. Else, in this order: `LeftPath = pair.0`, `RightPath = pair.1`, `SelectedPath = initial_selected_path.unwrap_or(pair.0)`,
    ///    `IsVisible = true`, `LeftImage = None`, `RightImage = None`; returns `Await` with
    ///    [`CompareWork::LoadImages`] for both paths (both loaders are started before either result is awaited).
    pub fn begin_load(
        &mut self,
        pair: (String, String),
        token: i64,
        is_token_current: &dyn Fn(i64) -> bool,
        options: CompareLoadOptions,
        ctx: &FormatContext<'_>,
    ) -> CompareLoadStep {
        todo!()
    }

    /// The C# code after the `await` the load is parked at, up to the next `await` or the end.
    ///
    /// `Images`: a failed left loader gives `Done(Failed/Cancelled)` (the left error wins, as `Task.WhenAll` rethrows the first
    /// task's); then the right one; nothing is applied. Else token check (false gives `Superseded`), `LeftImage` and `RightImage`
    /// are set, then the size step: with `compare_size_enabled` and `get_file_size` the sizes come from it at once
    /// (no await, no token check); with `compare_size_enabled` and no `get_file_size` the next work is `StatSizes`; with
    /// sizes disabled both suffixes are empty. A known size gives `status::compare_size_suffix(bytes, ctx)`, an unknown one
    /// the empty string.
    ///
    /// `Sizes`: token check (false gives `Superseded`), then the suffixes as above.
    ///
    /// After the sizes: `LeftSizeText` and `RightSizeText` are set (also when empty), then the hash step. Start with
    /// `hash = status::compare_hash_text(None)`. With `compare_hash_enabled && hash_source_available`: if
    /// `is_raw_path(left) != is_raw_path(right)` the hash is `compare_hash_text(Some(false))` and NO hashing is asked
    /// (a JPEG and its RAW can never be identical); else the next work is `ComputeHashes`.
    ///
    /// `Hashes`: any `Cancelled` gives `Done(Cancelled)` BEFORE the token check; then token check (false gives `Superseded`);
    /// then `Failed` on either side gives `status::compare_hash_unknown`, else `compare_hash_text(Some(eq_ignore_case(left, right)))` (`pr_core::text::eq_ignore_case`)
    /// (ordinal, ignoring case).
    ///
    /// End: `HashText = hash`, then `StatusText = status::compare(CompareStatus { index: current_index, count: total_files,
    /// left_name: file name of left, left_size, right_name, right_size, hash_text: HashText })` and `Done(Completed)`.
    /// File names use `pr_core::text::dotnet_path::get_file_name`.
    ///
    /// Calling it with a result that does not match `load.work()` is a programming error (`debug_assert!`; release returns
    /// `Done(Failed(CompareError::Failed(..)))`, never panics).
    pub fn resume_load(
        &mut self,
        load: CompareLoad,
        result: CompareWorkResult,
        is_token_current: &dyn Fn(i64) -> bool,
        ctx: &FormatContext<'_>,
    ) -> CompareLoadStep {
        todo!()
    }
}
