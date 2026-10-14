//! The two persisted `[Flags]` sets of the title bar and the photo information line.
//!
//! Port of `PhotoReview.Core/Model/TitleBarFields.cs` and `ExifInfoFields.cs` (baseline e441d67a), including their
//! `TitleBarFieldsJsonConverter` / `ExifInfoFieldsJsonConverter` (which only forward to `FlagEnumJson`, W1-02).
//! Contract: `contracts/W1-03/contract.md`.
//!
//! Each type is a newtype over the C# `int` bit values. The numeric values are persisted (config.json accepts numbers),
//! so they are fixed. JSON reading/writing is lenient and lives in `model::serde_ext` (`deserialize_flags`,
//! `serialize_flags`); the impls below only delegate.
//!
//! `Default::default()` here is the C# `Default` alias (`TitleBarFields::DEFAULT` / `ExifInfoFields::DEFAULT`), the value
//! `AppSettings` starts with and the fallback for unreadable JSON. It is NOT C# `default(T)`, which is `NONE`.

use std::ops::{BitAnd, BitOr, BitOrAssign};

use crate::model::serde_ext::{FlagEnum, deserialize_flags, serialize_flags};

/// Implements the shared flag-set API for a newtype `Name(u32)`; the constants stay hand-written next to each type.
macro_rules! flag_set {
    ($name:ident, $type_name:literal, names = [$(($text:literal, $value:expr)),+ $(,)?]) => {
        impl $name {
            /// The raw bit pattern (the C# enum's `int` value).
            pub const fn bits(self) -> u32 {
                self.0
            }

            /// `(T)Enum.ToObject(...)` after masking: keeps only the bits that belong to `ALL`.
            pub const fn from_bits_truncate(bits: u32) -> Self {
                Self(bits & Self::ALL.0)
            }

            /// C# `HasFlag`: true when every bit of `other` is set in `self` (always true for `NONE`).
            pub const fn contains(self, other: Self) -> bool {
                self.0 & other.0 == other.0
            }

            /// C# `a | b`.
            pub const fn union(self, other: Self) -> Self {
                Self(self.0 | other.0)
            }

            /// C# `a & ~b`: the bits of `self` that are not in `other`.
            pub const fn difference(self, other: Self) -> Self {
                Self(self.0 & !other.0)
            }
        }

        impl Default for $name {
            /// The C# `Default` alias, not C# `default(T)`.
            fn default() -> Self {
                Self::DEFAULT
            }
        }

        impl BitOr for $name {
            type Output = Self;
            fn bitor(self, rhs: Self) -> Self {
                self.union(rhs)
            }
        }

        impl BitOrAssign for $name {
            fn bitor_assign(&mut self, rhs: Self) {
                *self = self.union(rhs);
            }
        }

        impl BitAnd for $name {
            type Output = Self;
            fn bitand(self, rhs: Self) -> Self {
                Self(self.0 & rhs.0)
            }
        }

        impl FlagEnum for $name {
            const TYPE_NAME: &'static str = $type_name;
            const ALL_BITS: u32 = Self::ALL.0;
            const DEFAULT_BITS: u32 = Self::DEFAULT.0;
            const NAMES: &'static [(&'static str, u32)] = &[$(($text, $value)),+];

            fn from_bits(bits: u32) -> Self {
                Self::from_bits_truncate(bits)
            }

            fn bits(self) -> u32 {
                self.0
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serialize_flags(self, serializer)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                deserialize_flags(deserializer)
            }
        }
    };
}

/// Which parts are shown in the main window's title bar (in this fixed order, joined with " · "), on top of the
/// "Photo Review" app name. Written to config.json as comma-separated names (`"FolderName, FileName"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TitleBarFields(u32);

impl TitleBarFields {
    pub const NONE: Self = Self(0);
    pub const FOLDER_NAME: Self = Self(1);
    pub const FOLDER_PATH: Self = Self(2);
    pub const INDEX_COUNT: Self = Self(4);
    pub const FILE_NAME: Self = Self(8);
    pub const FILE_SIZE: Self = Self(16);
    pub const DIMENSIONS: Self = Self(32);
    pub const MODIFIED_DATE: Self = Self(64);
    pub const DATE_TAKEN: Self = Self(128);
    pub const CAMERA: Self = Self(256);
    pub const LENS: Self = Self(512);
    pub const ISO: Self = Self(1024);
    pub const FOCAL_LENGTH: Self = Self(2048);
    pub const APERTURE: Self = Self(4096);
    pub const SHUTTER_SPEED: Self = Self(8192);
    /// Every single flag (C# `All`, 16383).
    pub const ALL: Self = Self(16383);
    /// `AppSettings` default: the folder's own name only (C# `Default = FolderName`).
    pub const DEFAULT: Self = Self::FOLDER_NAME;
}

flag_set!(
    TitleBarFields,
    "TitleBarFields",
    names = [
        ("None", 0),
        ("FolderName", 1),
        ("FolderPath", 2),
        ("IndexCount", 4),
        ("FileName", 8),
        ("FileSize", 16),
        ("Dimensions", 32),
        ("ModifiedDate", 64),
        ("DateTaken", 128),
        ("Camera", 256),
        ("Lens", 512),
        ("Iso", 1024),
        ("FocalLength", 2048),
        ("Aperture", 4096),
        ("ShutterSpeed", 8192),
        ("All", 16383),
        ("Default", 1),
    ]
);

/// Which parts of the photo information line (under the status line) are shown. Written to config.json as
/// comma-separated names (`"FileName, Camera"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExifInfoFields(u32);

impl ExifInfoFields {
    pub const NONE: Self = Self(0);
    pub const FILE_NAME: Self = Self(1);
    pub const DATE_TAKEN: Self = Self(2);
    pub const DIMENSIONS: Self = Self(4);
    pub const CAMERA: Self = Self(8);
    pub const LENS: Self = Self(16);
    pub const ISO: Self = Self(32);
    pub const FOCAL_LENGTH: Self = Self(64);
    pub const APERTURE: Self = Self(128);
    pub const SHUTTER_SPEED: Self = Self(256);
    /// File last-write time (not a camera EXIF value); rendered like `DATE_TAKEN`, right after it.
    pub const MODIFIED_DATE: Self = Self(512);
    /// Every single flag (C# `All`, 1023).
    pub const ALL: Self = Self(1023);
    /// `AppSettings` default: every EXIF field except `FILE_NAME`, `DIMENSIONS` and `MODIFIED_DATE` (C# `All & ~(...)`, 506).
    pub const DEFAULT: Self = Self::ALL
        .difference(Self::FILE_NAME)
        .difference(Self::DIMENSIONS)
        .difference(Self::MODIFIED_DATE);
}

flag_set!(
    ExifInfoFields,
    "ExifInfoFields",
    names = [
        ("None", 0),
        ("FileName", 1),
        ("DateTaken", 2),
        ("Dimensions", 4),
        ("Camera", 8),
        ("Lens", 16),
        ("Iso", 32),
        ("FocalLength", 64),
        ("Aperture", 128),
        ("ShutterSpeed", 256),
        ("ModifiedDate", 512),
        ("All", 1023),
        ("Default", 506),
    ]
);
