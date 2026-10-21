//! Port of `PhotoReview.Shell.Rendering/Text/DWriteTextRenderer.cs` (201 lines, baseline `d92d6aae`): the DirectWrite
//! `ITextRenderer` and its lease `DWriteTextLayoutLease`. Unit W8-12i (test kinds F + N).
//!
//! Convention (C# class doc): [`TextTrimming::CharacterEllipsis`] = ONE line, cut by character with an ellipsis when the text is wider than
//! `max_width` (like `TextBlock TextTrimming=CharacterEllipsis` without wrap); [`TextTrimming::None`] with a finite `max_width` = wrap at word
//! boundaries at `max_width`; an infinite / NaN / huge `max_width` = unbounded (one line, no cut). Layouts are device independent
//! (measured in DIP), so a lost Direct2D device never touches the cache.
//!
//! The inherent API keeps the contract's interim types ([`TextError`], [`LayoutSize`], the `TextStyle` fields as plain arguments);
//! `impl TextRenderer for DWriteTextRenderer` and `impl TextLayout for DWriteTextLayoutLease` (layering follow-up, D-W8B1-1) adapt them
//! to `pr_app::services::render` (`TextError` -> `RenderError`, `LayoutSize` -> `SizeD`). [`TextTrimming`] is the `pr_app` type.
//!
//! UI thread only. `DWriteTextRenderer` and its leases are `!Send` (windows-rs 0.62.2 marks the DirectWrite interfaces `Send`, so
//! `PhantomData<*const ()>` must make the types `!Send`, D-W8B3G-9 d).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;

use pr_app::input::primitives::SizeD;
use pr_app::services::render::{RenderError, TextLayout, TextRenderer, TextStyle};
use windows::Win32::Graphics::DirectWrite::{DWRITE_WORD_WRAPPING_NO_WRAP, IDWriteFactory, IDWriteTextLayout};

pub use super::layout_cache::TextTrimming;
use super::layout_cache::{AcquireError, NewLayoutEntry, TextLayoutCache, TextLayoutEntry, TextLayoutKey};
use crate::gfx::com::{GfxError, ResultExt};
use crate::gfx::{dwrite, extras};

/// C# `DefaultCacheCapacity`: layouts kept in the LRU cache.
pub const DEFAULT_CACHE_CAPACITY: i32 = 256;
/// C# `Unbounded` (`1_000_000f`): the width that stands for "no limit" (DirectWrite needs a finite number). Equals `gfx::dwrite::UNBOUNDED`.
pub const UNBOUNDED: f32 = 1_000_000.0;
/// C# `style.FontFamily` replacement when it is null / empty / white space.
pub const DEFAULT_FONT_FAMILY: &str = "Segoe UI";
/// C# fallback locale when no UI culture name is available.
pub const FALLBACK_LOCALE: &str = "en-us";
/// `ObjectDisposedException.ObjectName` of the renderer.
pub const RENDERER_OBJECT_NAME: &str = "PhotoReview.Shell.Rendering.DWriteTextRenderer";

/// Twin of `SizeD` (DIP); converts into it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutSize {
    /// Width in DIP.
    pub width: f64,
    /// Height in DIP.
    pub height: f64,
}

impl From<LayoutSize> for SizeD {
    fn from(size: LayoutSize) -> Self {
        SizeD {
            width: size.width,
            height: size.height,
        }
    }
}

/// Twin of the `RenderError` variants of this file; converts into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextError {
    /// `ObjectDisposedException` of the renderer: `object` is [`RENDERER_OBJECT_NAME`].
    Disposed {
        /// The C# object name.
        object: &'static str,
    },
    /// `ArgumentOutOfRangeException(nameof(style), FontSizeDip, ..)`: `value_bits` is `f64::to_bits` of the size
    /// (`RenderError::InvalidFontSize`).
    InvalidFontSize {
        /// `f64::to_bits` of the rejected size.
        value_bits: u64,
    },
    /// A DirectWrite call failed (`COMException`).
    Device(GfxError),
}

impl From<TextError> for RenderError {
    fn from(error: TextError) -> Self {
        match error {
            TextError::Disposed { object } => RenderError::Disposed { object },
            TextError::InvalidFontSize { value_bits } => RenderError::InvalidFontSize { value_bits },
            TextError::Device(GfxError { hresult, operation }) => RenderError::Device { hresult, operation },
        }
    }
}

impl fmt::Display for TextError {
    /// The same texts as `RenderError` (C# exception messages).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&RenderError::from(*self), f)
    }
}

impl std::error::Error for TextError {}

/// C# `NormalizeWidth(double)`: NaN or `>= UNBOUNDED` (including +infinity) gives [`UNBOUNDED`]; `<= 0` (negative, zero, `-0.0`, -infinity)
/// gives `0.0` (positive zero); otherwise `maxWidth as f32` (round to nearest, like the C# cast). Pure.
#[must_use]
pub fn normalize_width(max_width: f64) -> f32 {
    if max_width.is_nan() || max_width >= f64::from(UNBOUNDED) {
        UNBOUNDED
    } else if max_width <= 0.0 {
        0.0
    } else {
        // `0 < max_width < 1_000_000`: the cast rounds to nearest and cannot overflow.
        max_width as f32
    }
}

/// C# `!(style.FontSizeDip > 0) || double.IsInfinity(style.FontSizeDip)` negated: true for a finite, strictly positive size. NaN, 0, negative
/// and +infinity are invalid. Pure.
#[must_use]
pub fn is_valid_font_size(font_size_dip: f64) -> bool {
    font_size_dip > 0.0 && font_size_dip.is_finite()
}

/// C# `string.IsNullOrWhiteSpace(style.FontFamily) ? "Segoe UI" : style.FontFamily`. Pure.
#[must_use]
pub fn effective_font_family(font_family: &str) -> &str {
    if font_family.trim().is_empty() {
        DEFAULT_FONT_FAMILY
    } else {
        font_family
    }
}

/// C# constructor locale rule: `localeName` if it is not null / white space, else `ui_culture_name` if it is not empty, else `"en-us"`. The
/// UI culture name comes from the localization service (C# `CultureInfo.CurrentUICulture.Name`). Pure.
#[must_use]
pub fn resolve_locale(locale_name: Option<&str>, ui_culture_name: &str) -> String {
    match locale_name {
        Some(name) if !name.trim().is_empty() => name.to_owned(),
        _ if !ui_culture_name.is_empty() => ui_culture_name.to_owned(),
        _ => FALLBACK_LOCALE.to_owned(),
    }
}

/// C# `Build(key)`: format, layout, the ellipsis recipe for `CharacterEllipsis`, metrics, `IsTrimmed`.
fn build_layout(
    factory: &IDWriteFactory,
    locale: &str,
    key: &TextLayoutKey,
) -> Result<NewLayoutEntry<IDWriteTextLayout>, GfxError> {
    let format = dwrite::create_text_format(factory, key.font_family(), key.bold(), key.font_size_dip(), locale)?;
    let layout = dwrite::create_text_layout(factory, key.text(), &format, key.max_width())?;
    let ellipsis = key.trimming() == TextTrimming::CharacterEllipsis;
    if ellipsis {
        // SAFETY: `layout` is a live owned `IDWriteTextLayout` created above; the argument is a plain enum constant, no pointer
        // escapes; called on the UI thread that owns the layout.
        unsafe { layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP) }.gfx("IDWriteTextLayout::SetWordWrapping")?;
        extras::set_character_ellipsis_trimming(factory, &layout)?;
    }
    let metrics = dwrite::layout_metrics(&layout)?;
    let is_trimmed = ellipsis && metrics.line_count >= 1 && dwrite::first_line_is_trimmed(&layout);
    Ok(NewLayoutEntry {
        width: f64::from(metrics.width),
        height: f64::from(metrics.height),
        line_count: i32::try_from(metrics.line_count).unwrap_or(i32::MAX),
        is_trimmed,
        native: layout,
    })
}

/// The DirectWrite renderer (C# `DWriteTextRenderer`).
pub struct DWriteTextRenderer {
    /// `None` once disposed (the shared factory is released immediately, like the C# `Marshal.Release`).
    factory: RefCell<Option<IDWriteFactory>>,
    locale: String,
    cache: Rc<TextLayoutCache<IDWriteTextLayout>>,
    disposed: Cell<bool>,
    _not_send: PhantomData<*const ()>,
}

impl fmt::Debug for DWriteTextRenderer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DWriteTextRenderer")
            .field("locale", &self.locale)
            .field("disposed", &self.disposed.get())
            .field("cache", &self.cache)
            .finish()
    }
}

impl DWriteTextRenderer {
    /// C# `DWriteTextRenderer(localeName, cacheCapacity)` (the parameterless C# constructor is `new(None, ui_culture_name, 256)`).
    /// Creates the shared DirectWrite factory (`gfx::dwrite::create_factory`). `cache_capacity < 1` is clamped to 1 (C# threw; the
    /// contract leaves the clamp to the caller, and a constructor with no error variant for it cannot fail).
    pub fn new(locale_name: Option<&str>, ui_culture_name: &str, cache_capacity: i32) -> Result<Self, TextError> {
        let cache = TextLayoutCache::new(cache_capacity.max(1))
            .map_err(|_| TextError::Device(GfxError::new("TextLayoutCache::new", -2_147_024_809)))?;
        let factory = dwrite::create_factory().map_err(TextError::Device)?;
        Ok(Self {
            factory: RefCell::new(Some(factory)),
            locale: resolve_locale(locale_name, ui_culture_name),
            cache: Rc::new(cache),
            disposed: Cell::new(false),
            _not_send: PhantomData,
        })
    }

    /// C# `CacheHits` (`CreateLayout` calls that found the layout).
    #[must_use]
    pub fn cache_hits(&self) -> i64 {
        self.cache.hits()
    }

    /// C# `CacheMisses`.
    #[must_use]
    pub fn cache_misses(&self) -> i64 {
        self.cache.misses()
    }

    /// C# `CachedLayoutCount`.
    #[must_use]
    pub fn cached_layout_count(&self) -> usize {
        self.cache.count()
    }

    /// C# `PendingReleaseCount`: evicted layouts that a lease still holds.
    #[must_use]
    pub fn pending_release_count(&self) -> i32 {
        self.cache.pending_release()
    }

    /// C# `CreateLayout(text, style, maxWidth, trimming)` with the `TextStyle` fields as arguments. Order of checks (C#): renderer disposed
    /// -> `Err(Disposed)`; `!is_valid_font_size` -> `Err(InvalidFontSize)`. Then the key (`effective_font_family`, `font_size_dip as f32`,
    /// `normalize_width`) and `cache.acquire`; the lease takes over the reference.
    pub fn create_layout(
        &self,
        text: &str,
        font_family: &str,
        font_size_dip: f64,
        bold: bool,
        max_width: f64,
        trimming: TextTrimming,
    ) -> Result<DWriteTextLayoutLease, TextError> {
        let disposed = TextError::Disposed {
            object: RENDERER_OBJECT_NAME,
        };
        if self.disposed.get() {
            return Err(disposed);
        }
        if !is_valid_font_size(font_size_dip) {
            return Err(TextError::InvalidFontSize {
                value_bits: font_size_dip.to_bits(),
            });
        }
        let factory = self.factory.borrow().clone().ok_or(disposed)?;
        // A finite `f64` size above `f32::MAX` becomes +infinity and a tiny one 0.0; DirectWrite rejects both with E_INVALIDARG.
        let key = TextLayoutKey::new(
            text,
            effective_font_family(font_family),
            font_size_dip as f32,
            bold,
            normalize_width(max_width),
            trimming,
        );
        let locale = self.locale.as_str();
        let entry = self
            .cache
            .acquire(key, |key| build_layout(&factory, locale, key))
            .map_err(|error| match error {
                AcquireError::Disposed => disposed,
                AcquireError::Build(error) => TextError::Device(error),
            })?;
        Ok(DWriteTextLayoutLease {
            text: entry.text().to_owned(),
            width: entry.width(),
            height: entry.height(),
            line_count: entry.line_count(),
            is_trimmed: entry.is_trimmed(),
            entry: Some(entry),
            cache: Rc::clone(&self.cache),
            _not_send: PhantomData,
        })
    }

    /// C# `Dispose()`: idempotent; disposes the cache (leased layouts stay alive until their last lease is dropped) and releases the factory.
    pub fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.cache.dispose();
        let factory = self.factory.borrow_mut().take();
        drop(factory);
    }
}

impl TextRenderer for DWriteTextRenderer {
    fn create_layout(
        &self,
        text: &str,
        style: &TextStyle,
        max_width: f64,
        trimming: TextTrimming,
    ) -> Result<Box<dyn TextLayout>, RenderError> {
        let lease = DWriteTextRenderer::create_layout(
            self,
            text,
            &style.font_family,
            style.font_size_dip,
            style.bold,
            max_width,
            trimming,
        )?;
        Ok(Box::new(lease))
    }
}

/// A lease on a cached layout (C# `DWriteTextLayoutLease`). Dropping it returns the reference (`TextLayoutCache::release`); the C#
/// `Dispose()` and `IsDisposed` have no twin (a dropped lease no longer exists). Implements `pr_app` `TextLayout`.
pub struct DWriteTextLayoutLease {
    text: String,
    width: f64,
    height: f64,
    line_count: i32,
    is_trimmed: bool,
    entry: Option<Rc<TextLayoutEntry<IDWriteTextLayout>>>,
    cache: Rc<TextLayoutCache<IDWriteTextLayout>>,
    _not_send: PhantomData<*const ()>,
}

impl fmt::Debug for DWriteTextLayoutLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DWriteTextLayoutLease")
            .field("text", &self.text)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("line_count", &self.line_count)
            .field("is_trimmed", &self.is_trimmed)
            .finish()
    }
}

impl DWriteTextLayoutLease {
    /// C# `Size` (DIP), measured once at build time.
    #[must_use]
    pub fn size(&self) -> LayoutSize {
        LayoutSize {
            width: self.width,
            height: self.height,
        }
    }

    /// C# `Text` (the original string).
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
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

    /// C# `NativeLayout` (the `INativeTextLayout` seam of `D2DDrawContext.DrawText`, W8-12e): a clone (`AddRef`) of the shared native layout.
    /// Always `Some` while the lease exists (the lease holds a reference, so the entry cannot have been released).
    #[must_use]
    pub fn native_layout(&self) -> Option<IDWriteTextLayout> {
        self.entry.as_ref()?.with_native(|native| native.cloned())
    }

    /// C# `SharedEntry`: true when both leases point at the same cached entry (tests: same key shares one layout).
    #[must_use]
    pub fn shares_entry_with(&self, other: &DWriteTextLayoutLease) -> bool {
        match (&self.entry, &other.entry) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl TextLayout for DWriteTextLayoutLease {
    fn size(&self) -> SizeD {
        DWriteTextLayoutLease::size(self).into()
    }

    fn text(&self) -> &str {
        DWriteTextLayoutLease::text(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Drop for DWriteTextLayoutLease {
    fn drop(&mut self) {
        if let Some(entry) = self.entry.take() {
            self.cache.release(&entry);
        }
    }
}
