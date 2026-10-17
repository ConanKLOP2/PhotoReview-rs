//! Port of `PhotoReview.Imaging.Raw/Tiff/DngContainerReader.cs` (C# baseline d92d6aae, 217 lines).
//!
//! Adobe DNG container reader (I-13). Walks IFD0, the IFD chain and the SubIFDs, lists the lossy-JPEG previews
//! (IFD JPEG-interchange pointer, or a single compression-6/7 strip whose first frame is SOF0/1/2) and picks the
//! sensor size from the raw IFD (CFA / LinearRaw first, then the largest), honouring `DefaultCropSize`.
//!
//! Uses the shared W5-01 contracts and W5-02 TIFF/JPEG navigators; behavior follows the W5-06 contract and C# baseline.
use std::collections::HashSet;

use pr_core::sync::CancelToken;
use pr_core::text::eq_ignore_case;
use pr_imaging::tiff::try_read_header;

use crate::formats::limits::MAX_IFD_COUNT;
use crate::jpeg_probe;
use crate::tiff_nav as nav;
use crate::{
    EmbeddedPreview, EmbeddedPreviewKind, PreviewColorSpace, RawContainerInfo, RawContainerReader, RawFormat,
    RawHeaderError, RawHeaderSource,
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
        eq_ignore_case(extension, ".dng")
            && first_64_bytes.len() >= 4
            && ((first_64_bytes[0..4] == [0x49, 0x49, 0x2A, 0x00])
                || (first_64_bytes[0..4] == [0x4D, 0x4D, 0x00, 0x2A]))
    }

    /// Reads the container info through bounded header reads.
    ///
    /// Errors: `RawHeaderError::InvalidData("File too short for DNG container.")` below 16 bytes,
    /// `InvalidData("Invalid DNG TIFF header.")` for a bad byte order or magic != 42, `Canceled` between IFDs, and every
    /// error of the header source / W5-02 helpers unchanged (an I/O failure stays an I/O failure).
    fn read(&self, source: &mut dyn RawHeaderSource, ct: &CancelToken) -> Result<RawContainerInfo, RawHeaderError> {
        if source.length() < 16 {
            return Err(RawHeaderError::invalid_data("File too short for DNG container."));
        }
        let header = source.read(0, 16)?;
        let Some(tiff) = try_read_header(header) else {
            return Err(RawHeaderError::invalid_data("Invalid DNG TIFF header."));
        };
        if tiff.magic != 42 {
            return Err(RawHeaderError::invalid_data("Invalid DNG TIFF header."));
        }
        let ifd0_offset = i64::from(tiff.ifd0_offset);
        let ifds = scan_ifd_chain(source, ct, tiff.little_endian, ifd0_offset)?;
        let mut orientation = 1;
        let mut sensor_width = 0;
        let mut sensor_height = 0;
        let mut primary_area = 0i32;
        let mut primary_is_raw = false;
        let mut previews = Vec::new();

        for ifd_offset in ifds.offsets {
            ct.check()?;
            let parsed = parse_ifd(source, tiff.little_endian, ifd0_offset, ifd_offset)?;
            if let Some(value) = parsed.orientation {
                orientation = value;
            }
            if let Some((offset, length, width, height)) = parsed.preview {
                previews.push(EmbeddedPreview::new(
                    previews.len() as i32,
                    offset,
                    length,
                    EmbeddedPreviewKind::Jpeg,
                    width,
                    height,
                    PreviewColorSpace::Unknown,
                ));
            } else if let Some((width, height)) = parsed.candidate {
                let area = (i64::from(width) * i64::from(height)).min(i64::from(i32::MAX)) as i32;
                if width > 0
                    && height > 0
                    && ((parsed.is_raw && !primary_is_raw) || (parsed.is_raw == primary_is_raw && area > primary_area))
                {
                    primary_area = area;
                    primary_is_raw = parsed.is_raw;
                    sensor_width = width;
                    sensor_height = height;
                }
            }
        }

        if sensor_width == 0 {
            if let Some(largest) = largest_preview(&previews) {
                sensor_width = largest.width;
                sensor_height = largest.height;
            }
        }
        let exif = nav::compute_exif_block(source, tiff.little_endian, ifd0_offset)?;
        Ok(RawContainerInfo {
            format: RawFormat::Dng,
            sensor_width,
            sensor_height,
            orientation,
            previews,
            exif_blocks: vec![exif],
        })
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
    let mut offsets = vec![ifd0_offset];
    let mut visited = HashSet::new();
    let mut current = ifd0_offset;
    let mut chain_count = 0i32;
    let mut sub_ifd_count = 0i32;
    while current > 0 && chain_count < MAX_IFD_COUNT {
        ct.check()?;
        if !visited.insert(current) {
            break;
        }
        chain_count += 1;
        let result = nav::read_ifd_entries(source, current, little_endian)?;
        for entry in &result.entries {
            if entry.tag != TAG_SUB_IFDS {
                continue;
            }
            let subs = nav::read_tag_unsigned_array(source, entry, little_endian, MAX_IFD_COUNT)?;
            for sub in subs {
                if sub > 0 && sub_ifd_count < MAX_IFD_COUNT && !offsets.contains(&sub) {
                    offsets.push(sub);
                    sub_ifd_count += 1;
                }
            }
        }
        let next = i64::from(result.next_ifd_offset);
        if next > 0 && !offsets.contains(&next) {
            offsets.push(next);
        }
        current = next;
    }
    Ok(IfdOffsets { offsets })
}

/// What step 2 learned from one IFD.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedIfd {
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
) -> Result<ParsedIfd, RawHeaderError> {
    let entries_read = nav::read_ifd_entries(source, ifd_offset, little_endian)?;
    let entries = entries_read.entries;
    let mut sub_file_type = 0u32;
    let mut compression = 1u16;
    let mut photometric = 0i64;
    let mut width = 0i32;
    let mut height = 0i32;
    let mut jpeg_offset = None;
    let mut jpeg_length = None;
    let mut orientation = None;

    for entry in &entries {
        match entry.tag {
            TAG_NEW_SUB_FILE_TYPE => {
                if let Some(value) = nav::read_tag_unsigned(source, entry, little_endian)? {
                    sub_file_type = value as u32;
                }
            }
            TAG_IMAGE_WIDTH => width = nav::clamp_to_int(nav::read_tag_unsigned(source, entry, little_endian)?),
            TAG_IMAGE_LENGTH => height = nav::clamp_to_int(nav::read_tag_unsigned(source, entry, little_endian)?),
            TAG_PHOTOMETRIC => {
                if let Some(value) = nav::read_tag_unsigned(source, entry, little_endian)? {
                    photometric = value;
                }
            }
            TAG_COMPRESSION => {
                if let Some(value) = nav::read_tag_unsigned(source, entry, little_endian)? {
                    compression = if (0..=i64::from(u16::MAX)).contains(&value) {
                        value as u16
                    } else {
                        0
                    };
                }
            }
            TAG_ORIENTATION if ifd_offset == ifd0_offset => {
                if let Some(value @ 1..=8) = nav::read_tag_unsigned(source, entry, little_endian)? {
                    orientation = Some(value as i32);
                }
            }
            TAG_JPEG_INTERCHANGE_FORMAT => jpeg_offset = nav::read_tag_unsigned(source, entry, little_endian)?,
            TAG_JPEG_INTERCHANGE_FORMAT_LENGTH => jpeg_length = nav::read_tag_unsigned(source, entry, little_endian)?,
            _ => {}
        }
    }

    let preview = if let (Some(offset), Some(length)) = (jpeg_offset, jpeg_length) {
        if nav::starts_with_soi(source, offset, length)? {
            let (mut preview_width, mut preview_height) = (width, height);
            nav::reconcile_jpeg_size(
                source,
                offset,
                length,
                &mut preview_width,
                &mut preview_height,
                |src, off, len| {
                    jpeg_probe::try_read_lossy_frame(src, off, len)
                        .map(|frame| frame.map(|value| (value.width, value.height)))
                },
            )?;
            Some((offset, length, preview_width, preview_height))
        } else {
            None
        }
    } else {
        None
    };
    let preview = if preview.is_some() {
        preview
    } else if compression == 6 || compression == 7 {
        if let Some((offset, length)) = nav::try_read_single_strip(source, &entries, little_endian)? {
            jpeg_probe::try_read_lossy_frame(source, offset, length)?
                .map(|frame| (offset, length, frame.width, frame.height))
        } else {
            None
        }
    } else {
        None
    };

    let is_raw = photometric == PHOTOMETRIC_CFA || photometric == PHOTOMETRIC_LINEAR_RAW;
    let candidate = if preview.is_none() && sub_file_type & SECONDARY_IMAGE_MASK == 0 {
        let mut candidate = (width, height);
        if let Some((crop_width, crop_height)) = nav::try_read_default_crop_size(source, &entries, little_endian)? {
            if nav::has_square_default_scale(source, &entries, little_endian)? {
                candidate = nav::choose_active_sensor_size(width, height, crop_width, crop_height, 0, 0);
            }
        }
        Some(candidate)
    } else {
        None
    };
    Ok(ParsedIfd {
        orientation,
        preview,
        candidate,
        is_raw,
    })
}

/// Index of the preview with the largest `width * height` (i64 product), the first one on a tie.
fn largest_preview(previews: &[EmbeddedPreview]) -> Option<&EmbeddedPreview> {
    let mut largest = None;
    let mut largest_area = i64::MIN;
    for preview in previews {
        let area = i64::from(preview.width) * i64::from(preview.height);
        if area > largest_area {
            largest = Some(preview);
            largest_area = area;
        }
    }
    largest
}
