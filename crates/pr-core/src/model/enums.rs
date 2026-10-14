//! The persisted Model enums.
//!
//! Port of `PhotoReview.Core/Model/{ImageSortMode,InitialViewMode,LoadingMode,DecoderBackend,RawPairMode,
//! RawFullDecode,MouseWheelAction,ScalingQuality,ImageTransition,InstanceMode,JournalState,FileOperationType,
//! JournalDurability,FitWidthAnchor,KeyboardZoomAnchor,KineticGlideSmoothing}.cs` (baseline e441d67a), including
//! `SortModePolicy` from `ImageSortMode.cs`. Contract: `contracts/W1-01/contract.md`.
//!
//! Every enum here carries `[JsonConverter(typeof(LenientEnumConverter<T>))]` in C#. The `model_enum!` macro below gives
//! each one the same three things: the [`LenientEnum`] impl (names, numeric values, `[JsonAlias]` aliases, written
//! text), and `serde` impls that delegate to `model::serde_ext` (W1-02). Numeric values are persisted (config.json,
//! journal), so they are fixed: never renumber or reorder.
//!
//! `Default::default()` is C# `default(T)`, the member with value 0. That is NOT always the `AppSettings` default
//! (for example `ImageSortMode::default()` is `Name`, the setting defaults to `Default`); the settings module owns those.

// Stub: only `uses_explorer_order` still has a `todo!()` body. The porter deletes this line with it.
#![allow(unused_variables)]

use crate::model::serde_ext::LenientEnum;

/// Declares one persisted enum plus its `LenientEnum`, `Default`, `Serialize` and `Deserialize` impls.
///
/// Per variant, in this order: doc comments, any number of `#[alias("a", "b")]` (the C# `[JsonAlias]`), an optional
/// `#[text("100%")]` (the text `LenientEnumConverter<T>.ToText` writes when it is not the variant name), then
/// `Name = value`.
macro_rules! model_enum {
    (@text $default:expr) => { $default };
    (@text $default:expr, $over:expr) => { $over };
    (
        $(#[$meta:meta])*
        $name:ident {
            $(
                $(#[doc = $doc:literal])*
                $(#[alias($($alias:literal),+ $(,)?)])*
                $(#[text($text:literal)])?
                $variant:ident = $value:literal
            ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[repr(i32)]
        pub enum $name {
            $(
                $(#[doc = $doc])*
                $variant = $value
            ),+
        }

        impl LenientEnum for $name {
            const TYPE_NAME: &'static str = stringify!($name);
            const MEMBERS: &'static [Self] = &[$(Self::$variant),+];

            fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => stringify!($variant)),+
                }
            }

            fn value(self) -> i32 {
                self as i32
            }

            fn aliases(self) -> &'static [&'static str] {
                match self {
                    $(Self::$variant => &[$($($alias,)+)*]),+
                }
            }

            fn json_text(self) -> &'static str {
                match self {
                    $(Self::$variant => model_enum!(@text stringify!($variant) $(, $text)?)),+
                }
            }

            fn zero() -> Self {
                Self::MEMBERS
                    .iter()
                    .copied()
                    .find(|member| member.value() == 0)
                    .unwrap_or(Self::MEMBERS[0])
            }
        }

        impl Default for $name {
            /// C# `default(T)`: the member with value 0.
            fn default() -> Self {
                <Self as LenientEnum>::zero()
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                $crate::model::serde_ext::serialize_lenient(self, serializer)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                $crate::model::serde_ext::deserialize_lenient(deserializer)
            }
        }
    };
}

model_enum! {
    /// How the image list is sorted (`ImageSortMode`).
    ImageSortMode {
        /// The app's natural name order (same order as `NameAscending`); follows the Explorer view order when a window shows the folder.
        #[alias("PortraitFirst")]
        Name = 0,
        /// Largest first; follows the Explorer view order.
        #[alias("Size")]
        SizeDescending = 1,
        /// Smallest first; follows the Explorer view order.
        SizeAscending = 2,
        /// The catalog keeps the order the file system enumerates the folder in (no sort, no Explorer order).
        Default = 3,
        /// The app's own natural name order, A to Z (same order as `Name`), never Explorer's order.
        NameAscending = 4,
        /// Exactly the reverse of `NameAscending`, never Explorer's order.
        NameDescending = 5
    }
}

model_enum! {
    /// Initial zoom/fit mode when an image is opened (`InitialViewMode`). Written as `Fit`, `100%`, `200%`, `400%`
    /// (legacy-compatible), otherwise as the variant name.
    InitialViewMode {
        Fit = 0,
        #[alias("100%")]
        #[text("100%")]
        Percent100 = 1,
        #[alias("200%")]
        #[text("200%")]
        Percent200 = 2,
        /// Legacy value kept only so old configs still parse; the settings normalizer migrates a loaded value of this to `Percent200`.
        #[alias("400%")]
        #[text("400%")]
        Percent400 = 3,
        /// Fills the viewport width exactly; vertical placement follows `FitWidthAnchor`.
        #[alias("fit-width")]
        FitWidth = 4,
        /// Fills the viewport height exactly, centred horizontally and vertically.
        #[alias("fit-height")]
        FitHeight = 5,
        /// Zooms straight to the click-zoom percent, centred in the viewport.
        #[alias("click-zoom")]
        ClickZoomLevel = 6
    }
}

model_enum! {
    /// How an image is loaded when shown or browsed (`LoadingMode`).
    LoadingMode {
        Fast = 0,
        Preview = 1,
        Original = 2
    }
}

model_enum! {
    /// The image decoder backend (`DecoderBackend`). `LibRaw` is internal only (the settings normalizer resets it).
    DecoderBackend {
        Wpf = 0,
        WicDirect = 1,
        TurboJpeg = 2,
        LibRaw = 3
    }
}

model_enum! {
    /// Handling of JPG+RAW pairs (`RawPairMode`, Q-RAW-04).
    RawPairMode {
        Separate = 0,
        PreferJpeg = 1,
        PreferRaw = 2
    }
}

model_enum! {
    /// Full decode behaviour for RAW images (`RawFullDecode`, Q-RAW-02).
    RawFullDecode {
        Never = 0,
        OnZoom = 1
    }
}

model_enum! {
    /// What the mouse wheel does over the main image (`MouseWheelAction`). Ctrl+wheel always zooms at the cursor.
    MouseWheelAction {
        /// Default: each wheel event zooms one step, anchored at the cursor.
        Zoom = 0,
        /// Wheel down = next image, wheel up = previous image (one image per notch of 120).
        Navigate = 1
    }
}

model_enum! {
    /// Interpolation / scaling quality (`ScalingQuality`).
    ScalingQuality {
        Linear = 0,
        HighQuality = 1
    }
}

model_enum! {
    /// Transition effect when the VIEWED image changes (`ImageTransition`); never applied to same-image upgrades.
    ImageTransition {
        /// No effect: a hard cut (default, zero cost).
        None = 0,
        /// The old image fades out while the new one is already shown underneath.
        Fade = 1
    }
}

model_enum! {
    /// How many processes may run per Windows user session (`InstanceMode`, Q-R18). Read once at startup.
    InstanceMode {
        /// Default: one app-wide lock and forward pipe; every later launch hands its path to the running window.
        SingleWindow = 0,
        /// One window per folder; the lock and forward pipe follow the folder the window currently shows.
        PerFolder = 1
    }
}

model_enum! {
    /// State of an operation in the journal (`JournalState`).
    JournalState {
        Prepared = 0,
        Committed = 1,
        Failed = 2,
        /// The user removed the entry from the Recovery list; no file operation happened.
        Dismissed = 3
    }
}

model_enum! {
    /// Kind of file operation done during review (`FileOperationType`). The old name `Delete` is still read.
    FileOperationType {
        Move = 0,
        Copy = 1,
        #[alias("Delete")]
        Recycle = 2
    }
}

model_enum! {
    /// How hard the operation journal pushes each record to disk (`JournalDurability`, ADR 0007, IO03).
    JournalDurability {
        /// Default (J-C): plain flush into the OS cache; survives a process crash, not a power loss.
        Fast = 0,
        /// J-D: write-through plus a full flush per record, journal and mutation off the UI thread.
        PowerLossSafe = 1
    }
}

model_enum! {
    /// Vertical anchor used by `InitialViewMode::FitWidth` (`FitWidthAnchor`).
    FitWidthAnchor {
        /// Image point (0.5, 0.5) at the viewport centre. Default.
        Centre = 0,
        /// Image point (0.5, 1/3) at the viewport centre; portraits usually have the face there.
        TopThird = 1
    }
}

model_enum! {
    /// What point stays put when keyboard zoom, 100 % or the click-zoom shortcut changes the zoom (`KeyboardZoomAnchor`).
    KeyboardZoomAnchor {
        /// Anchor at the mouse position when it is over the image viewport; the viewport centre otherwise.
        Pointer = 0,
        /// Always anchor at the viewport centre, whatever the mouse position.
        ViewportCentre = 1
    }
}

model_enum! {
    /// How the kinetic glide after a drag-pan is timed against the display (`KineticGlideSmoothing`).
    KineticGlideSmoothing {
        /// Each frame advances by the time between the render timestamps (the original behaviour).
        Off = 0,
        /// Each frame advances to the refresh (vblank) of the monitor it is expected to be shown on.
        Predict = 1
    }
}

/// `SortModePolicy.UsesExplorerOrder`: true for the modes where an open Explorer window's view order replaces the
/// app's own sort (`Name`, `SizeDescending`, `SizeAscending`). `Default`, `NameAscending` and `NameDescending` are
/// decided by the app: the Explorer query is not even started.
pub fn uses_explorer_order(mode: ImageSortMode) -> bool {
    todo!()
}
