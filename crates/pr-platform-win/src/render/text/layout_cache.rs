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
//! [`TextTrimming`] is the `pr_app::services::render::TextTrimming` type itself (the contract's placeholder was removed with the layering
//! follow-up, D-W8B3S-4).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::rc::Rc;

pub use pr_app::services::render::TextTrimming;

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
        Self {
            text: text.to_owned(),
            font_family: font_family.to_owned(),
            font_size_bits: font_size_dip.to_bits(),
            bold,
            max_width_bits: max_width.to_bits(),
            trimming,
        }
    }

    /// C# `Text`.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// C# `FontFamily`.
    #[must_use]
    pub fn font_family(&self) -> &str {
        &self.font_family
    }

    /// C# `FontSizeDip`.
    #[must_use]
    pub fn font_size_dip(&self) -> f32 {
        f32::from_bits(self.font_size_bits)
    }

    /// C# `Bold`.
    #[must_use]
    pub fn bold(&self) -> bool {
        self.bold
    }

    /// C# `MaxWidth`.
    #[must_use]
    pub fn max_width(&self) -> f32 {
        f32::from_bits(self.max_width_bits)
    }

    /// C# `Trimming`.
    #[must_use]
    pub fn trimming(&self) -> TextTrimming {
        self.trimming
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
        f.debug_struct("TextLayoutEntry")
            .field("key", &self.key)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("line_count", &self.line_count)
            .field("is_trimmed", &self.is_trimmed)
            .field("ref_count", &self.ref_count.get())
            .field("evicted", &self.evicted.get())
            .field("released", &self.is_released())
            .finish()
    }
}

impl<N> TextLayoutEntry<N> {
    /// C# `Key`.
    #[must_use]
    pub fn key(&self) -> &TextLayoutKey {
        &self.key
    }

    /// C# `Text`.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.key.text
    }

    /// C# `Size` width in DIP.
    #[must_use]
    pub fn width(&self) -> f64 {
        self.width
    }

    /// C# `Size` height in DIP.
    #[must_use]
    pub fn height(&self) -> f64 {
        self.height
    }

    /// C# `LineCount`.
    #[must_use]
    pub fn line_count(&self) -> i32 {
        self.line_count
    }

    /// C# `IsTrimmed`.
    #[must_use]
    pub fn is_trimmed(&self) -> bool {
        self.is_trimmed
    }

    /// C# `RefCount`.
    #[must_use]
    pub fn ref_count(&self) -> i32 {
        self.ref_count.get()
    }

    /// C# `Evicted`.
    #[must_use]
    pub fn is_evicted(&self) -> bool {
        self.evicted.get()
    }

    /// C# `IsReleased`: the native layout was dropped.
    #[must_use]
    pub fn is_released(&self) -> bool {
        self.native.borrow().is_none()
    }

    /// C# `Native`: runs `f` with the native layout; `None` when released (C# throws `ObjectDisposedException`).
    pub fn with_native<R>(&self, f: impl FnOnce(Option<&N>) -> R) -> R {
        f(self.native.borrow().as_ref())
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
    /// `stamp -> key`, ascending: the FIRST element is the least recently used end (C# `_lru.Last`); a hit or a new entry takes a new,
    /// larger stamp (C# `AddFirst`). Eviction removes the SMALLEST stamp.
    order: BTreeMap<u64, TextLayoutKey>,
    next_stamp: u64,
    hits: i64,
    misses: i64,
    /// C# `_pending`: entries evicted but still leased.
    pending: i32,
    disposed: bool,
}

impl<N> State<N> {
    fn take_stamp(&mut self) -> u64 {
        let stamp = self.next_stamp;
        self.next_stamp = self.next_stamp.wrapping_add(1);
        stamp
    }

    /// Removes the least recently used entry from both maps (smallest stamp).
    fn evict_oldest(&mut self) -> Option<Rc<TextLayoutEntry<N>>> {
        let (_, key) = self.order.pop_first()?;
        let (_, entry) = self.map.remove(&key)?;
        Some(entry)
    }
}

/// The LRU cache with reference-counted entries (C# `TextLayoutCache`). Shared as `Rc` by the renderer and every lease.
pub struct TextLayoutCache<N> {
    capacity: i32,
    state: RefCell<State<N>>,
}

impl<N> fmt::Debug for TextLayoutCache<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.borrow();
        f.debug_struct("TextLayoutCache")
            .field("capacity", &self.capacity)
            .field("count", &state.map.len())
            .field("hits", &state.hits)
            .field("misses", &state.misses)
            .field("pending", &state.pending)
            .field("disposed", &state.disposed)
            .finish()
    }
}

impl<N> TextLayoutCache<N> {
    /// C# constructor: `capacity >= 1`.
    pub fn new(capacity: i32) -> Result<Self, InvalidCapacity> {
        if capacity < 1 {
            return Err(InvalidCapacity(capacity));
        }
        Ok(Self {
            capacity,
            state: RefCell::new(State {
                map: HashMap::new(),
                order: BTreeMap::new(),
                next_stamp: 0,
                hits: 0,
                misses: 0,
                pending: 0,
                disposed: false,
            }),
        })
    }

    /// C# `Count`: layouts currently cached.
    #[must_use]
    pub fn count(&self) -> usize {
        self.state.borrow().map.len()
    }

    /// C# `Hits`.
    #[must_use]
    pub fn hits(&self) -> i64 {
        self.state.borrow().hits
    }

    /// C# `Misses`.
    #[must_use]
    pub fn misses(&self) -> i64 {
        self.state.borrow().misses
    }

    /// C# `PendingRelease`: evicted entries that a lease still holds.
    #[must_use]
    pub fn pending_release(&self) -> i32 {
        self.state.borrow().pending
    }

    /// C# `Acquire(key, factory)`: a hit makes the entry most recent and increments `ref_count`; a miss counts first (so a failing `build`
    /// still counts), calls `build` (no borrow of the cache state is held during the call), stores the entry with `ref_count = 1` and evicts
    /// from the least recent end while `count > capacity` (the new entry is never the victim). The caller must [`release`](Self::release).
    pub fn acquire<E>(
        &self,
        key: TextLayoutKey,
        build: impl FnOnce(&TextLayoutKey) -> Result<NewLayoutEntry<N>, E>,
    ) -> Result<Rc<TextLayoutEntry<N>>, AcquireError<E>> {
        {
            let mut state = self.state.borrow_mut();
            if state.disposed {
                return Err(AcquireError::Disposed);
            }
            let hit = state.map.get(&key).map(|(stamp, entry)| (*stamp, Rc::clone(entry)));
            if let Some((old_stamp, entry)) = hit {
                state.hits += 1;
                let stamp = state.take_stamp();
                state.order.remove(&old_stamp);
                state.order.insert(stamp, key.clone());
                if let Some(slot) = state.map.get_mut(&key) {
                    slot.0 = stamp;
                }
                entry.ref_count.set(entry.ref_count.get().saturating_add(1));
                return Ok(entry);
            }
            // The miss is counted before `build`, so a failing build still counts (T2).
            state.misses += 1;
        }
        // No borrow of the state is held while `build` runs.
        let parts = build(&key).map_err(AcquireError::Build)?;
        let entry = Rc::new(TextLayoutEntry {
            key: key.clone(),
            width: parts.width,
            height: parts.height,
            line_count: parts.line_count,
            is_trimmed: parts.is_trimmed,
            ref_count: Cell::new(1),
            evicted: Cell::new(false),
            native: RefCell::new(Some(parts.native)),
        });
        let mut victims = Vec::new();
        {
            let mut state = self.state.borrow_mut();
            let stamp = state.take_stamp();
            state.order.insert(stamp, key.clone());
            state.map.insert(key, (stamp, Rc::clone(&entry)));
            let capacity = usize::try_from(self.capacity).unwrap_or(usize::MAX);
            while state.map.len() > capacity {
                match state.evict_oldest() {
                    Some(victim) => victims.push(victim),
                    None => break,
                }
            }
        }
        // Payloads are dropped outside the state borrow.
        for victim in victims {
            self.finish_eviction(&victim);
        }
        Ok(entry)
    }

    /// C# `Release(entry)`: `ref_count <= 0` is ignored; otherwise decrements, and an evicted entry that reaches 0 drops its native layout
    /// and decrements `pending`. Works after [`dispose`](Self::dispose) (leases outlive the renderer).
    pub fn release(&self, entry: &Rc<TextLayoutEntry<N>>) {
        if entry.ref_count.get() <= 0 {
            return;
        }
        entry.ref_count.set(entry.ref_count.get() - 1);
        if entry.evicted.get() && entry.ref_count.get() == 0 && !entry.is_released() {
            let payload = entry.native.borrow_mut().take();
            {
                let mut state = self.state.borrow_mut();
                state.pending = state.pending.saturating_sub(1);
            }
            drop(payload);
        }
    }

    /// C# `Clear()`: evicts everything (leased entries become pending).
    pub fn clear(&self) {
        self.evict_all();
    }

    /// C# `Dispose()`: evicts everything, then refuses further `acquire`; idempotent.
    pub fn dispose(&self) {
        self.evict_all();
        self.state.borrow_mut().disposed = true;
    }

    /// C# `EvictAll`.
    fn evict_all(&self) {
        let victims: Vec<Rc<TextLayoutEntry<N>>> = {
            let mut state = self.state.borrow_mut();
            state.order.clear();
            state.map.drain().map(|(_, (_, entry))| entry).collect()
        };
        for victim in victims {
            self.finish_eviction(&victim);
        }
    }

    /// The tail of an eviction (the entry is already out of both maps): an unleased entry drops its payload now, a leased one becomes
    /// pending until its last lease is returned.
    fn finish_eviction(&self, entry: &Rc<TextLayoutEntry<N>>) {
        entry.evicted.set(true);
        if entry.ref_count.get() == 0 {
            let payload = entry.native.borrow_mut().take();
            drop(payload);
        } else {
            let mut state = self.state.borrow_mut();
            state.pending = state.pending.saturating_add(1);
        }
    }
}
