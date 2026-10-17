//! Canon CR2 container reader (port of `PhotoReview.Imaging.Raw/Tiff/Cr2ContainerReader.cs`, W5-03, ADR 0009).
//!
//! CR2 is a little-endian TIFF with the two bytes `CR` at offset 8 (the Canon marker sits where classic TIFF has the
//! first IFD entry count, so IFD0 is normally at offset 16):
//!
//! * IFD0 carries the FULL-SIZE JPEG preview as a single strip (`StripOffsets` 0x0111 / `StripByteCounts` 0x0117); IFD0 is
//!   NOT the raw image, so its `ImageWidth`/`ImageLength` are the preview size (sRAW: larger than the sensor data).
//! * IFD1 (and later IFDs) may carry a smaller JPEG through `JPEGInterchangeFormat` 0x0201 / Length 0x0202.
//! * Orientation is read from IFD0 only (0x0112).
//! * Sensor size = Exif `PixelXDimension`/`PixelYDimension` of the first Exif IFD that has both, else the largest IFD.
//!
//! Hostile-input safe: the IFD chain is capped at [`MAX_IFD_COUNT`] and cycle-checked, and every failure is a
//! [`RawHeaderError`] (`InvalidData`, or `Canceled`), never a panic. Pure parser: no file system, no clock, no `unsafe`.
//! Header bytes come only through [`RawHeaderSource`], so the 8 MiB header budget of the source is honoured.
//!
//! Depends on W5-01 (types, traits, limits), W5-02 (`TiffHeaderNavigator`) and `pr_imaging::tiff` (W4-03a).

use std::collections::HashSet;

use pr_core::sync::CancelToken;
use pr_imaging::tiff::try_read_header;

use crate::formats::limits;
use crate::preview_selector::try_read_jpeg_frame;
use crate::tiff_nav;
use crate::{
    EmbeddedPreview, EmbeddedPreviewKind, ExifBlock, PreviewColorSpace, RawContainerInfo, RawContainerReader,
    RawFormat, RawHeaderError, RawHeaderSource,
};

/// `RawContainerLimits.MaxIfdCount`: IFDs visited by one `read` (C# `const int`, kept `i32`). Shared with every reader.
pub const MAX_IFD_COUNT: i32 = limits::MAX_IFD_COUNT;

/// A CR2 file must hold at least the 16 bytes `read` fetches first (C# `source.Length < 16`).
pub const MIN_FILE_LENGTH: i64 = 16;

/// The TIFF magic `read` requires (the sniffing `can_read` also requires `II`; `read` accepts `MM` too).
pub const TIFF_MAGIC: u16 = 42;

/// `Orientation` tag, read from IFD0 only.
pub const TAG_ORIENTATION: u16 = 0x0112;

/// `ExifIFD` pointer tag.
pub const TAG_EXIF_IFD: u16 = 0x8769;

/// Stateless reader for Canon `.cr2` files (C# `Cr2ContainerReader`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cr2ContainerReader;

impl Cr2ContainerReader {
    pub const fn new() -> Self {
        Self
    }
}

/// JPEG frame size used by `ReconcileJpegSize`: the frame header of the JPEG at `offset`
/// (`PreviewSelector.TryReadJpegFrame`), `None` when there is no readable frame.
fn read_jpeg_frame_size(
    source: &mut dyn RawHeaderSource,
    offset: i64,
    length: i64,
) -> Result<Option<(i32, i32)>, RawHeaderError> {
    Ok(try_read_jpeg_frame(source, offset, length)?.map(|frame| (frame.width, frame.height)))
}

/// A preview of CR2 (always a JPEG with an unknown colour space, header not yet resolved).
fn jpeg_preview(index: i32, offset: i64, length: i64, width: i32, height: i32) -> EmbeddedPreview {
    EmbeddedPreview::new(
        index,
        offset,
        length,
        EmbeddedPreviewKind::Jpeg,
        width,
        height,
        PreviewColorSpace::Unknown,
    )
}

impl RawContainerReader for Cr2ContainerReader {
    /// `true` when `extension` equals `.cr2` (OrdinalIgnoreCase, via `pr_core::text::eq_ignore_case`) AND
    /// `first_64_bytes` holds at least 10 bytes that start `49 49 2A 00` (`II*\0`, little-endian only) and have `C` `R`
    /// at offsets 8 and 9. Bytes 4..8 (the IFD0 offset) are not looked at.
    fn can_read(&self, first_64_bytes: &[u8], extension: &str) -> bool {
        if !pr_core::text::eq_ignore_case(extension, ".cr2") {
            return false;
        }
        matches!(first_64_bytes, [0x49, 0x49, 0x2A, 0x00, _, _, _, _, b'C', b'R', ..])
    }

    /// Parses the container through bounded header reads.
    ///
    /// Errors: `RawHeaderError::InvalidData` for a source shorter than 16 bytes, an unreadable TIFF header, a magic other
    /// than 42, and whatever the navigator or the source raises (range outside the source, exhausted header budget,
    /// I/O failure wrapped by the source); `RawHeaderError::Canceled` when `ct` is canceled at the top of an IFD iteration.
    /// Never panics.
    fn read(&self, source: &mut dyn RawHeaderSource, ct: &CancelToken) -> Result<RawContainerInfo, RawHeaderError> {
        if source.length() < MIN_FILE_LENGTH {
            return Err(RawHeaderError::invalid_data("File too short for CR2 container."));
        }

        let header = try_read_header(source.read(0, 16)?)
            .ok_or_else(|| RawHeaderError::invalid_data("Invalid CR2 TIFF header."))?;
        if header.magic != TIFF_MAGIC {
            return Err(RawHeaderError::invalid_data("Invalid CR2 TIFF header."));
        }
        let little_endian = header.little_endian;
        let ifd0_offset = i64::from(header.ifd0_offset);

        let mut orientation = 1i32;
        let mut exif_width = 0i32;
        let mut exif_height = 0i32;
        let mut largest_area = 0i64;
        let mut largest_width = 0i32;
        let mut largest_height = 0i32;
        let mut previews: Vec<EmbeddedPreview> = Vec::new();
        let mut exif_blocks: Vec<ExifBlock> = Vec::new();

        // Visited IFD offsets guard against cycles; order never leaks, so a set is enough.
        let mut visited: HashSet<i64> = HashSet::new();
        let mut current_ifd_offset = ifd0_offset;
        let mut ifd_index: i32 = 0;

        while current_ifd_offset > 0 && ifd_index < MAX_IFD_COUNT {
            ct.check()?;
            if !visited.insert(current_ifd_offset) {
                break; // cycle detected
            }

            let ifd = tiff_nav::read_ifd_entries(source, current_ifd_offset, little_endian)?;
            let entries = ifd.entries;
            let next_ifd_offset = ifd.next_ifd_offset;
            let (ifd_width, ifd_height) = tiff_nav::read_image_size(source, &entries, little_endian)?;

            if ifd_index == 0 {
                if let Some(value) = tiff_nav::read_tag_value(source, &entries, TAG_ORIENTATION, little_endian)? {
                    if let Ok(value) = i32::try_from(value) {
                        if (1..=8).contains(&value) {
                            orientation = value;
                        }
                    }
                }
            }

            if let Some(exif_offset) = tiff_nav::read_tag_value(source, &entries, TAG_EXIF_IFD, little_endian)? {
                if exif_offset > 0 {
                    // ComputeExifBlock always describes the TIFF header (offset 0) from IFD0, so one block covers every IFD.
                    if exif_blocks.is_empty() {
                        exif_blocks.push(tiff_nav::compute_exif_block(source, little_endian, ifd0_offset)?);
                    }
                    if exif_width == 0 {
                        if let Some((pixel_width, pixel_height)) =
                            tiff_nav::try_read_exif_pixel_dimensions(source, exif_offset, little_endian)?
                        {
                            exif_width = pixel_width;
                            exif_height = pixel_height;
                        }
                    }
                }
            }

            let area = i64::from(ifd_width) * i64::from(ifd_height);
            if area > largest_area {
                largest_area = area;
                largest_width = ifd_width;
                largest_height = ifd_height;
            }

            if ifd_index == 0 {
                // Full size preview in IFD0 StripOffsets/ByteCounts (a JPEG split over several strips is rejected).
                if let Some((strip_offset, strip_length)) =
                    tiff_nav::try_read_single_strip(source, &entries, little_endian)?
                {
                    if tiff_nav::starts_with_soi(source, strip_offset, strip_length)? {
                        let (mut strip_width, mut strip_height) = (ifd_width, ifd_height);
                        tiff_nav::reconcile_jpeg_size(
                            source,
                            strip_offset,
                            strip_length,
                            &mut strip_width,
                            &mut strip_height,
                            read_jpeg_frame_size,
                        )?;
                        let index = i32::try_from(previews.len()).unwrap_or(i32::MAX);
                        previews.push(jpeg_preview(
                            index,
                            strip_offset,
                            strip_length,
                            strip_width,
                            strip_height,
                        ));
                    }
                }
            } else if let Some((jpeg_offset, jpeg_length)) =
                tiff_nav::try_read_jpeg_interchange(source, &entries, little_endian)?
            {
                // Secondary preview / thumbnail (IFD1, etc.)
                let (mut preview_width, mut preview_height) = (ifd_width, ifd_height);
                tiff_nav::reconcile_jpeg_size(
                    source,
                    jpeg_offset,
                    jpeg_length,
                    &mut preview_width,
                    &mut preview_height,
                    read_jpeg_frame_size,
                )?;
                let index = i32::try_from(previews.len()).unwrap_or(i32::MAX);
                previews.push(jpeg_preview(
                    index,
                    jpeg_offset,
                    jpeg_length,
                    preview_width,
                    preview_height,
                ));
            }

            current_ifd_offset = i64::from(next_ifd_offset);
            ifd_index += 1;
        }

        // IFD0 is only a JPEG preview (for sRAW it is the full-size preview, larger than the sensor data), and the
        // raw IFD may lack ImageWidth/Length. The Exif pixel size is the real image size; else the largest IFD.
        let (sensor_width, sensor_height) = if exif_width > 0 {
            (exif_width, exif_height)
        } else {
            (largest_width, largest_height)
        };

        if exif_blocks.is_empty() {
            exif_blocks.push(tiff_nav::compute_exif_block(source, little_endian, ifd0_offset)?);
        }

        Ok(RawContainerInfo {
            format: RawFormat::Cr2,
            sensor_width,
            sensor_height,
            orientation,
            previews,
            exif_blocks,
        })
    }
}
