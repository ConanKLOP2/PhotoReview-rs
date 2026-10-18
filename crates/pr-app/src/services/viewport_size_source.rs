//! Port of `Services/ViewportSizeSource.cs` (52 lines). Contract: `contracts/W8-04c.md`, rules R-VS-*.
//!
//! Two values are published lock-free from any thread: the preview decode box (read by preload workers that build cache keys off
//! the UI thread) and the startup prediction (diagnostics). The viewport accessor is set once by the window after its first layout.
//! Each box is packed into ONE 64-bit atomic (width in the high 32 bits, height in the low 32 bits), so a reader never pairs the
//! width of one box with the height of another (C# `Volatile` on one `long`). No `unsafe`: `AtomicI64` only.

use std::fmt;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, RwLock};

use pr_imaging::decode_types::DecodeBox;

/// The viewport accessor: returns the client size `(width, height)` in device-independent units (C# `Func<(double, double)>`).
pub type ViewportSizeFn = Arc<dyn Fn() -> (f64, f64) + Send + Sync>;

/// Packed value of "no startup prediction" (R-VS-4): every negative packed value reads back as `None`.
const NO_PREDICTION: i64 = -1;

/// C# `ViewportSizeSource` (52 lines).
pub struct ViewportSizeSource {
    /// C# `Get`: default `() => (0, 0)`. `RwLock`, not `OnceLock`, because the window may replace it (R-VS-5).
    get: RwLock<ViewportSizeFn>,
    /// C# `_packedTargetDecodeBox`: packed `DecodeBox` (see [`pack_box`]). Default packs `DecodeBox::UNBOUNDED`.
    packed_target_decode_box: AtomicI64,
    /// C# `_packedStartupPrediction`: `-1` = no prediction (R-VS-4).
    packed_startup_prediction: AtomicI64,
}

impl ViewportSizeSource {
    /// Default state: viewport `(0, 0)`, target box unbounded, no startup prediction (R-VS-1).
    pub fn new() -> Self {
        let get: ViewportSizeFn = Arc::new(|| (0.0, 0.0));
        Self {
            get: RwLock::new(get),
            packed_target_decode_box: AtomicI64::new(pack_box(DecodeBox::UNBOUNDED)),
            packed_startup_prediction: AtomicI64::new(NO_PREDICTION),
        }
    }

    /// C# `Get()`: the current client size from the installed accessor (`(0, 0)` until the window installs one).
    pub fn viewport_size(&self) -> (f64, f64) {
        // The accessor is cloned out so the lock is not held while it runs (it may read the window).
        let get = match self.get.read() {
            Ok(guard) => Arc::clone(&guard),
            Err(poisoned) => Arc::clone(&poisoned.into_inner()),
        };
        get()
    }

    /// C# `Get` setter: installs the window's accessor (R-VS-5). Takes `&self`: the source is shared with the window.
    pub fn set_viewport_size_fn(&self, get: ViewportSizeFn) {
        match self.get.write() {
            Ok(mut guard) => *guard = get,
            Err(poisoned) => *poisoned.into_inner() = get,
        }
    }

    /// C# `TargetDecodeBox` getter: `DecodeBox` published with [`Self::set_target_decode_box`]; lock-free (R-VS-2).
    pub fn target_decode_box(&self) -> DecodeBox {
        unpack_box(self.packed_target_decode_box.load(Ordering::Acquire))
    }

    /// C# `TargetDecodeBox` setter: publishes width and height together (R-VS-2).
    pub fn set_target_decode_box(&self, value: DecodeBox) {
        self.packed_target_decode_box.store(pack_box(value), Ordering::Release);
    }

    /// C# `StartupPrediction` getter: `None` when the packed value is negative (R-VS-4).
    pub fn startup_prediction(&self) -> Option<DecodeBox> {
        let packed = self.packed_startup_prediction.load(Ordering::Acquire);
        // R-VS-4: a negative packed value (including the -1 sentinel) reads back as no prediction.
        (packed >= 0).then(|| unpack_box(packed))
    }

    /// C# `StartupPrediction` setter: `None` stores the `-1` sentinel (R-VS-4).
    pub fn set_startup_prediction(&self, value: Option<DecodeBox>) {
        let packed = value.map_or(NO_PREDICTION, pack_box);
        self.packed_startup_prediction.store(packed, Ordering::Release);
    }
}

impl Default for ViewportSizeSource {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ViewportSizeSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewportSizeSource")
            .field("target_decode_box", &self.target_decode_box())
            .field("startup_prediction", &self.startup_prediction())
            .finish_non_exhaustive()
    }
}

/// Packs a box as C# `((long)width << 32) | (uint)height` (R-VS-3). Private to the module; exposed to tests through the
/// setters/getters only.
fn pack_box(value: DecodeBox) -> i64 {
    // C# ((long)width << 32) | (uint)height. The shift never panics; the high word keeps the sign of the width.
    (i64::from(value.width) << 32) | i64::from(value.height as u32)
}

/// Unpacks as C# `new DecodeBox((int)(packed >> 32), unchecked((int)packed))` (R-VS-3: arithmetic shift, low 32 bits).
fn unpack_box(packed: i64) -> DecodeBox {
    // Arithmetic shift for the width, low 32 bits for the height (R-VS-3).
    DecodeBox::new((packed >> 32) as i32, packed as i32)
}

#[cfg(test)]
mod tests {
    // Contract unit tests (R-VS-3): private packing against fixed bit patterns; the module is the only place they can reach.
    use super::*;

    #[test]
    fn pack_unpack_matches_csharp_bits() {
        // (1, 2): width in the high 32 bits, height in the low 32 bits.
        assert_eq!(pack_box(DecodeBox::new(1, 2)), 0x0000_0001_0000_0002);
        assert_eq!(unpack_box(0x0000_0001_0000_0002), DecodeBox::new(1, 2));
    }

    #[test]
    fn pack_negative_height_uses_the_low_word() {
        // C# `(uint)height`: -5 is 0xFFFF_FFFB in the low 32 bits, width 0 leaves the high word clear.
        assert_eq!(pack_box(DecodeBox::new(0, -5)), 0x0000_0000_FFFF_FFFB);
        assert_eq!(unpack_box(0x0000_0000_FFFF_FFFB), DecodeBox::new(0, -5));
    }
}

#[cfg(test)]
mod debug_tests {
    use super::*;

    #[test]
    fn debug_output_names_the_type_and_fields() {
        let text = format!("{:?}", ViewportSizeSource::new());
        assert!(text.contains("ViewportSizeSource"), "{text}");
        assert!(text.contains("startup_prediction"), "{text}");
    }
}
