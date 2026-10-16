//! Port of `PhotoReview.Imaging.Raw/Tiff/DngContainerReader.cs` (C# baseline d92d6aae, 217 lines).
//!
//! Adobe DNG container reader (I-13). Walks IFD0, the IFD chain and the SubIFDs, lists the lossy-JPEG previews
//! (IFD JPEG-interchange pointer, or a single compression-6/7 strip whose first frame is SOF0/1/2) and picks the
//! sensor size from the raw IFD (CFA / LinearRaw first, then the largest), honouring `DefaultCropSize`.
//!
//! Stub: `todo!()` bodies, real signatures. The helpers it calls live in W5-01 (types, limits, traits) and W5-02
//! (`tiff_navigator`, `jpeg_probe`); see `contracts/W5-06.md` "Depends on" for the exact assumed signatures.
#![allow(unused_variables, unused_imports, dead_code)]

use pr_core::sync::CancelToken;

use crate::formats::limits::MAX_IFD_COUNT;
use crate::jpeg_probe;
use crate::tiff_nav::{self as nav, TiffEntry};
use crate::{
    EmbeddedPreview, EmbeddedPreviewKind, ExifBlock, PreviewColorSpace, RawContainerInfo, RawContainerReader,
    RawFormat, RawHeaderError, RawHeaderSource,
};

/// `NewSubFileType` bits that mark a secondary image: reduced resolution (0x1), transparency mask (0x4), 0x8, and the
/// DNG 1.6 enhanced / super-resolution image (0x10). Bit 0x2 (single page of a multi-page file) is NOT secondary.
const SECONDARY_IMAGE_MASK: u32 = 0x1 | 0x4 | 0x8 | 0x10;

/// `PhotometricInterpretation` values that mark the raw image: 32803 = CFA, 34892 = LinearRaw.
const PHOTOMETRIC_CFA: i64 = 32803;
const PHOTOMETRIC_LINEAR_RAW: i64 = 34892;

// TIFF / DNG tags this reader looks at.
const TAG_NEW_SUB_FILE_TYPE: u16 = 0x00FE;
const TAG_IMAGE_WIDTH: u16 = 0x0100;
const TAG_IMAGE_LENGTH: u16 = 0x0101;
const TAG_COMPRESSION: u16 = 0x0103;
const TAG_PHOTOMETRIC: u16 = 0x0106;
const TAG_ORIENTATION: u16 = 0x0112;
const TAG_SUB_IFDS: u16 = 0x014A;
const TAG_JPEG_INTERCHANGE_FORMAT: u16 = 0x0201;
const TAG_JPEG_INTERCHANGE_FORMAT_LENGTH: u16 = 0x0202;

/// Reader for Adobe DNG (`IRawContainerReader` implementation). Stateless: one value serves every file.
#[derive(Debug, Clone, Copy, Default)]
pub struct DngContainerReader;

impl DngContainerReader {
    /// `new DngContainerReader()`.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl RawContainerReader for DngContainerReader {
    /// True for extension `.dng` (OrdinalIgnoreCase) and a first-64-bytes block that starts `II*\0` or `MM\0*`
    /// (at least 4 bytes; magic is exactly 42 here, ORF/RW2 magics are not DNG).
    fn can_read(&self, first_64_bytes: &[u8], extension: &str) -> bool {
        todo!("W5-06: extension eq_ignore_case(\".dng\") && len >= 4 && (II 2A 00 | MM 00 2A)")
    }

    /// Reads the container info through bounded header reads.
    ///
    /// Errors: `RawHeaderError::InvalidData("File too short for DNG container.")` below 16 bytes,
    /// `InvalidData("Invalid DNG TIFF header.")` for a bad byte order or magic != 42, `Canceled` between IFDs, and every
    /// error of the header source / W5-02 helpers unchanged (an I/O failure stays an I/O failure).
    fn read(&self, source: &mut dyn RawHeaderSource, ct: &CancelToken) -> Result<RawContainerInfo, RawHeaderError> {
        todo!("W5-06: step 1 scan_ifd_chain, step 2 parse_ifd per offset, preview-size fallback, one EXIF block")
    }
}

/// Step 1 result: every IFD offset to parse, in discovery order (IFD0 first), duplicates removed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct IfdOffsets {
    /// IFD0, then each SubIFD / next-IFD pointer in the order it was first registered.
    offsets: Vec<i64>,
}

/// Step 1: follow the IFD chain (cap `MAX_IFD_COUNT`) and collect the SubIFD pointers of every chain IFD (separate cap
/// `MAX_IFD_COUNT`). The chain stops on a repeated offset; the last `next` pointer is still registered.
fn scan_ifd_chain(
    source: &mut dyn RawHeaderSource,
    ct: &CancelToken,
    little_endian: bool,
    ifd0_offset: i64,
) -> Result<IfdOffsets, RawHeaderError> {
    todo!("W5-06: port the while loop of DngContainerReader.Read")
}

/// What step 2 learned from one IFD.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedIfd {
    sub_file_type: u32,
    compression: u16,
    photometric: i64,
    width: i32,
    height: i32,
    /// `Some` only for IFD0 and only for a value in 1..=8; later entries override earlier ones.
    orientation: Option<i32>,
    /// A lossy-JPEG preview this IFD contributed (offset, length, width, height); `None` when it is not a preview.
    preview: Option<(i64, i64, i32, i32)>,
    /// Candidate raw size after `DefaultCropSize` (only when this IFD is neither a preview nor a secondary image).
    candidate: Option<(i32, i32)>,
    is_raw: bool,
}

/// Step 2 for one IFD offset (re-reads the entries, exactly like C#: the header budget is charged twice per IFD).
fn parse_ifd(
    source: &mut dyn RawHeaderSource,
    little_endian: bool,
    ifd0_offset: i64,
    ifd_offset: i64,
    preview_index: i32,
) -> Result<ParsedIfd, RawHeaderError> {
    todo!("W5-06: port the foreach (entry) switch and the preview / primary-image decisions")
}

/// Index of the preview with the largest `width * height` (i64 product), the first one on a tie.
fn largest_preview(previews: &[EmbeddedPreview]) -> Option<&EmbeddedPreview> {
    todo!("W5-06: LINQ OrderByDescending(..).First() is stable")
}
