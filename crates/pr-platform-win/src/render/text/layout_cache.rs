//! Port of `PhotoReview.Shell.Rendering/Text/TextLayoutCache.cs` (197 lines, baseline `d92d6aae`): `TextLayoutKey`, `TextLayoutEntry` and
//! `TextLayoutCache`. Unit W8-12i (pure part, test kind F).
//!
//! WP-17: an LRU cache of laid-out text keyed by everything that changes the DirectWrite result (text, font family, size, bold, max width,
//! trimming). A layout is device independent (measured in DIP), so a lost Direct2D device never invalidates the cache. The caller gets a
//! LEASE (reference count): evicting a layout that an overlay still holds is safe, the native object is released when it is both evicted
//! and its last lease is returned.
//!
//! Generic over the native payload `N` (`IDWriteTextLayout` in production, a drop-counting fake in the tests), so no `windows` type is
//! needed here. UI thread only: `Rc`/`RefCell`/`Cell`, no lock (the C# `lock` only protected a lease disposed from another thread; a Rust
//! lease is `!Send`).
//!
//! PLACEHOLDER (D-W8B3S-4): [`TextTrimming`] is a local copy of `pr_app::services::render::TextTrimming` (same discriminants) because this
//! crate cannot name `pr_app` yet; the porter replaces it by a `pub use` after the layering PR.

#![allow(unused_variables, dead_code)] // W8-12i stub: the porter removes this line when the last `todo!()` is gone.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::rc::Rc;

/// Placeholder twin of `pr_app::services::render::TextTrimming` (C# `TextTrimming`: `None` = 0, `CharacterEllipsis` = 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextTrimming {
    /// Wrap at the word boundary at `max_width` (unbounded width: one line).
    None = 0,
    /// One line, cut by character with an ellipsis sign.
    CharacterEllipsis = 1,
}

/// C# `record struct TextLayoutKey(string Text, string FontFamily, float FontSizeDip, bool Bold, float MaxWidth, TextTrimming Trimming)`.
/// `Eq`/`Hash` by value. The two `f32` are stored as bits: by construction they are never NaN and never `-0.0` (size is finite and
/// positive, the width is normalised by `normalize_width`), so bit equality equals the C# `float.Equals`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextLayoutKey {
    text: String,
    font_family: String,
    font_size_bits: u32,
    bold: bool,
    max_width_bits: u32,
    trimming: TextTrimming,
}

impl TextLayoutKey {
    /// Builds a key. `font_size_dip` and `max_width` are the already normalised `f32` values of the renderer.
    #[must_use]
    pub fn new(
        text: &str,
        font_family: &str,
        font_size_dip: f32,
        bold: bool,
        max_width: f32,
        trimming: TextTrimming,
    ) -> Self {
        todo!("W8-12i")
    }

    /// C# `Text`.
    #[must_use]
    pub fn text(&self) -> &str {
        todo!("W8-12i")
    }

    /// C# `FontFamily`.
    #[must_use]
    pub fn font_family(&self) -> &str {
        todo!("W8-12i")
    }

    /// C# `FontSizeDip`.
    #[must_use]
    pub fn font_size_dip(&self) -> f32 {
        todo!("W8-12i")
    }

    /// C# `Bold`.
    #[must_use]
    pub fn bold(&self) -> bool {
        todo!("W8-12i")
    }

    /// C# `MaxWidth`.
    #[must_use]
    pub fn max_width(&self) -> f32 {
        todo!("W8-12i")
    }

    /// C# `Trimming`.
    #[must_use]
    pub fn trimming(&self) -> TextTrimming {
        todo!("W8-12i")
    }
}

/// What `build` returns to [`TextLayoutCache::acquire`] (the C# `TextLayoutEntry` constructor arguments; key and text come from the key).
#[derive(Debug)]
pub struct NewLayoutEntry<N> {
    /// The native layout.
    pub native: N,
    /// `new SizeD(metrics.Width, metrics.Height)` (f32 widened to f64).
    pub width: f64,
    /// See `width`.
    pub height: f64,
    /// `(int)metrics.LineCount`.
    pub line_count: i32,
    /// `trimmed` flag (only ever true for `CharacterEllipsis`).
    pub is_trimmed: bool,
}

/// C# `TextLayoutEntry`: one built layout with its measurements and reference count. Shared as `Rc` between the cache and the leases.
pub struct TextLayoutEntry<N> {
    key: TextLayoutKey,
    width: f64,
    height: f64,
    line_count: i32,
    is_trimmed: bool,
    ref_count: Cell<i32>,
    evicted: Cell<bool>,
    /// `None` once released (C# `_native = null`).
    native: RefCell<Option<N>>,
}

impl<N> fmt::Debug for TextLayoutEntry<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("W8-12i")
    }
}

impl<N> TextLayoutEntry<N> {
    /// C# `Key`.
    #[must_use]
    pub fn key(&self) -> &TextLayoutKey {
        todo!("W8-12i")
    }

    /// C# `Text`.
    #[must_use]
    pub fn text(&self) -> &str {
        todo!("W8-12i")
    }

    /// C# `Size` width in DIP.
    #[must_use]
    pub fn width(&self) -> f64 {
        todo!("W8-12i")
    }

    /// C# `Size` height in DIP.
    #[must_use]
    pub fn height(&self) -> f64 {
        todo!("W8-12i")
    }

    /// C# `LineCount`.
    #[must_use]
    pub fn line_count(&self) -> i32 {
        todo!("W8-12i")
    }

    /// C# `IsTrimmed`.
    #[must_use]
    pub fn is_trimmed(&self) -> bool {
        todo!("W8-12i")
    }

    /// C# `RefCount`.
    #[must_use]
    pub fn ref_count(&self) -> i32 {
        todo!("W8-12i")
    }

    /// C# `Evicted`.
    #[must_use]
    pub fn is_evicted(&self) -> bool {
        todo!("W8-12i")
    }

    /// C# `IsReleased`: the native layout was dropped.
    #[must_use]
    pub fn is_released(&self) -> bool {
        todo!("W8-12i")
    }

    /// C# `Native`: runs `f` with the native layout; `None` when released (C# throws `ObjectDisposedException`).
    pub fn with_native<R>(&self, f: impl FnOnce(Option<&N>) -> R) -> R {
        todo!("W8-12i")
    }
}

/// `capacity < 1` (C# `ArgumentOutOfRangeException.ThrowIfLessThan(capacity, 1)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidCapacity(pub i32);

/// Failure of [`TextLayoutCache::acquire`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcquireError<E> {
    /// C# `ObjectDisposedException` of the cache (the renderer reports its own name instead, see W8-12i).
    Disposed,
    /// `build` failed; nothing was cached, the miss was counted.
    Build(E),
}

/// Everything behind the `RefCell`.
struct State<N> {
    map: HashMap<TextLayoutKey, (u64, Rc<TextLayoutEntry<N>>)>,
    /// `stamp -> key`, ascending: the LAST element is the least recently used end of the C# list (`_lru.Last`); a hit or a new entry takes a new,
    /// larger stamp (C# `AddFirst`). Eviction removes the SMALLEST stamp.
    order: BTreeMap<u64, TextLayoutKey>,
    next_stamp: u64,
    hits: i64,
    misses: i64,
    /// C# `_pending`: entries evicted but still leased.
    pending: i32,
    disposed: bool,
}

/// The LRU cache with reference-counted entries (C# `TextLayoutCache`). Shared as `Rc` by the renderer and every lease.
pub struct TextLayoutCache<N> {
    capacity: i32,
    state: RefCell<State<N>>,
}

impl<N> fmt::Debug for TextLayoutCache<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("W8-12i")
    }
}

impl<N> TextLayoutCache<N> {
    /// C# constructor: `capacity >= 1`.
    pub fn new(capacity: i32) -> Result<Self, InvalidCapacity> {
        todo!("W8-12i")
    }

    /// C# `Count`: layouts currently cached.
    #[must_use]
    pub fn count(&self) -> usize {
        todo!("W8-12i")
    }

    /// C# `Hits`.
    #[must_use]
    pub fn hits(&self) -> i64 {
        todo!("W8-12i")
    }

    /// C# `Misses`.
    #[must_use]
    pub fn misses(&self) -> i64 {
        todo!("W8-12i")
    }

    /// C# `PendingRelease`: evicted entries that a lease still holds.
    #[must_use]
    pub fn pending_release(&self) -> i32 {
        todo!("W8-12i")
    }

    /// C# `Acquire(key, factory)`: a hit makes the entry most recent and increments `ref_count`; a miss counts first (so a failing `build`
    /// still counts), calls `build` (no borrow of the cache state is held during the call), stores the entry with `ref_count = 1` and evicts
    /// from the least recent end while `count > capacity` (the new entry is never the victim). The caller must [`release`](Self::release).
    pub fn acquire<E>(
        &self,
        key: TextLayoutKey,
        build: impl FnOnce(&TextLayoutKey) -> Result<NewLayoutEntry<N>, E>,
    ) -> Result<Rc<TextLayoutEntry<N>>, AcquireError<E>> {
        todo!("W8-12i")
    }

    /// C# `Release(entry)`: `ref_count <= 0` is ignored; otherwise decrements, and an evicted entry that reaches 0 drops its native layout
    /// and decrements `pending`. Works after [`dispose`](Self::dispose) (leases outlive the renderer).
    pub fn release(&self, entry: &Rc<TextLayoutEntry<N>>) {
        todo!("W8-12i")
    }

    /// C# `Clear()`: evicts everything (leased entries become pending).
    pub fn clear(&self) {
        todo!("W8-12i")
    }

    /// C# `Dispose()`: evicts everything, then refuses further `acquire`; idempotent.
    pub fn dispose(&self) {
        todo!("W8-12i")
    }
}
