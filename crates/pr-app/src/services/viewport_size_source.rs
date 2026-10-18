//! Port of `Services/ViewportSizeSource.cs` (52 lines). Contract: `contracts/W8-04c.md`, rules R-VS-*.
//!
//! Two values are published lock-free from any thread: the preview decode box (read by preload workers that build cache keys off
//! the UI thread) and the startup prediction (diagnostics). The viewport accessor is set once by the window after its first layout.
//! Each box is packed into ONE 64-bit atomic (width in the high 32 bits, height in the low 32 bits), so a reader never pairs the
//! width of one box with the height of another (C# `Volatile` on one `long`). No `unsafe`: `AtomicI64` only.

#![allow(unused_variables, dead_code)] // D11: stub bodies are todo!(); removed with the last todo!() of this file.

use std::fmt;
use std::sync::atomic::AtomicI64;
use std::sync::{Arc, RwLock};

use pr_imaging::decode_types::DecodeBox;

/// The viewport accessor: returns the client size `(width, height)` in device-independent units (C# `Func<(double, double)>`).
pub type ViewportSizeFn = Arc<dyn Fn() -> (f64, f64) + Send + Sync>;

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
        todo!()
    }

    /// C# `Get()`: the current client size from the installed accessor (`(0, 0)` until the window installs one).
    pub fn viewport_size(&self) -> (f64, f64) {
        todo!()
    }

    /// C# `Get` setter: installs the window's accessor (R-VS-5). Takes `&self`: the source is shared with the window.
    pub fn set_viewport_size_fn(&self, get: ViewportSizeFn) {
        todo!()
    }

    /// C# `TargetDecodeBox` getter: `DecodeBox` published with [`Self::set_target_decode_box`]; lock-free (R-VS-2).
    pub fn target_decode_box(&self) -> DecodeBox {
        todo!()
    }

    /// C# `TargetDecodeBox` setter: publishes width and height together (R-VS-2).
    pub fn set_target_decode_box(&self, value: DecodeBox) {
        todo!()
    }

    /// C# `StartupPrediction` getter: `None` when the packed value is negative (R-VS-4).
    pub fn startup_prediction(&self) -> Option<DecodeBox> {
        todo!()
    }

    /// C# `StartupPrediction` setter: `None` stores the `-1` sentinel (R-VS-4).
    pub fn set_startup_prediction(&self, value: Option<DecodeBox>) {
        todo!()
    }
}

impl Default for ViewportSizeSource {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ViewportSizeSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

/// Packs a box as C# `((long)width << 32) | (uint)height` (R-VS-3). Private to the module; exposed to tests through the
/// setters/getters only.
fn pack_box(value: DecodeBox) -> i64 {
    todo!()
}

/// Unpacks as C# `new DecodeBox((int)(packed >> 32), unchecked((int)packed))` (R-VS-3: arithmetic shift, low 32 bits).
fn unpack_box(packed: i64) -> DecodeBox {
    todo!()
}
