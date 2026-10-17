//! Port target: `PhotoReview.Imaging.Raw` (ADR 0009). Ported parser, not a third-party RAW crate, so output stays identical.
//!
//! Wave 5 RAW container, reader, routing, and decoder implementations are pinned by `contracts/W5-*.md` and the C# goldens.

pub mod bmff;
pub mod contracts;
pub mod decoder;
pub mod formats;
pub mod header_source;
pub mod jpeg_probe;
pub mod preview_selector;
pub mod raw_exif;
pub mod readers;
pub mod registry;
pub mod routing;
pub mod tiff_nav;

pub use contracts::{
    EmbeddedPreview, EmbeddedPreviewKind, ExifBlock, InMemoryRawHeaderSource, PreviewColorSpace, RAW_HEADER_BLOCK_SIZE,
    RawContainerReader, RawHeaderError, RawHeaderSource, RawPreviewFallback,
};
pub use formats::{RAW_EXTENSIONS, RawContainerInfo, RawFormat, is_raw_extension};
