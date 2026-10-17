//! Test-only port of C# `RawOrientationPatcher` at baseline `d92d6aae`.
//!
//! Rewrites EXIF Orientation in TIFF-family RAW bytes, RAF/RW2 embedded JPEG EXIF, or a CR3 CMT1 TIFF block. This helper
//! only changes caller-owned synthetic/test bytes; it does not touch files or depend on LibRaw. It deliberately returns
//! `false` for malformed/out-of-range inputs where the C# helper can throw (D60).

use pr_imaging_raw::RawContainerInfo;

/// C# `RawOrientationPatcher.TryWriteOrientation`.
///
/// Routing is intentionally the same as the oracle: CR3 only uses CMT1; RAF skips TIFF-at-zero; other extensions first
/// try TIFF-at-zero, then all non-CR3 formats may fall back to the first preview's embedded JPEG EXIF.
pub fn try_write_orientation(file: &mut [u8], extension: &str, info: &RawContainerInfo, orientation: u16) -> bool {
    if extension == ".cr3" {
        return try_find_cr3_cmt1_tiff(file)
            .is_some_and(|tiff_start| try_write_tiff_orientation(file, tiff_start, orientation));
    }
    if extension != ".raf" && try_write_tiff_orientation(file, 0, orientation) {
        return true;
    }
    let Some(preview) = info.previews.first() else {
        return false;
    };
    let Ok(jpeg_start) = usize::try_from(preview.offset) else {
        return false;
    };
    let Some(mut offset) = jpeg_start.checked_add(2) else {
        return false;
    };

    loop {
        let Some(marker_end) = offset.checked_add(4) else {
            return false;
        };
        if marker_end > file.len() || file[offset] != 0xFF {
            break;
        }
        let marker = file[offset + 1];
        let length = usize::from(u16::from_be_bytes([file[offset + 2], file[offset + 3]]));
        let Some(exif_end) = offset.checked_add(10) else {
            return false;
        };
        if marker == 0xE1 && length >= 8 && file.get(offset + 4..exif_end) == Some(b"Exif\0\0".as_slice()) {
            return try_write_tiff_orientation(file, exif_end, orientation);
        }
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        let Some(next) = offset.checked_add(2).and_then(|at| at.checked_add(length)) else {
            return false;
        };
        offset = next;
    }
    false
}

/// C# `RawOrientationPatcher.TryFindCr3Cmt1Tiff`.
#[must_use]
pub fn try_find_cr3_cmt1_tiff(file: &[u8]) -> Option<usize> {
    let head = &file[..file.len().min(1 << 20)];
    let type_at = head.windows(4).position(|bytes| bytes == b"CMT1")?;
    let trailer_end = type_at.checked_add(12)?;
    if type_at < 4 || trailer_end > head.len() {
        return None;
    }
    if !matches!(head.get(type_at + 4..type_at + 6), Some(b"II" | b"MM")) {
        return None;
    }
    type_at.checked_add(4)
}

/// C# `RawOrientationPatcher.TryRenameIfd0Tag`.
pub fn try_rename_ifd0_tag(file: &mut [u8], tiff_start: usize, tag: u16, replacement: u16) -> bool {
    let Some((entry, little)) = find_ifd0_entry(file, tiff_start, tag) else {
        return false;
    };
    let bytes = if little {
        replacement.to_le_bytes()
    } else {
        replacement.to_be_bytes()
    };
    file[entry..entry + 2].copy_from_slice(&bytes);
    true
}

/// C# `RawOrientationPatcher.TryWriteTiffOrientation`.
pub fn try_write_tiff_orientation(file: &mut [u8], tiff_start: usize, orientation: u16) -> bool {
    let Some((entry, little)) = find_ifd0_entry(file, tiff_start, 0x0112) else {
        return false;
    };
    if read_u16(file, entry + 2, little) != Some(3) {
        return false;
    }
    let bytes = if little {
        orientation.to_le_bytes()
    } else {
        orientation.to_be_bytes()
    };
    file[entry + 8..entry + 10].copy_from_slice(&bytes);
    true
}

fn find_ifd0_entry(file: &[u8], tiff_start: usize, tag: u16) -> Option<(usize, bool)> {
    let header_end = tiff_start.checked_add(8)?;
    if header_end > file.len() {
        return None;
    }
    // The C# helper treats any first byte other than 'I' as big-endian; preserve that behavior.
    let little = file[tiff_start] == b'I';
    let ifd0 = tiff_start.checked_add(usize::try_from(read_u32(file, tiff_start + 4, little)?).ok()?)?;
    let count_end = ifd0.checked_add(2)?;
    if count_end > file.len() {
        return None;
    }
    let count = usize::from(read_u16(file, ifd0, little)?);
    for i in 0..count {
        let entry = i.checked_mul(12)?.checked_add(count_end)?;
        let entry_end = entry.checked_add(12)?;
        if entry_end > file.len() {
            break;
        }
        if read_u16(file, entry, little)? == tag {
            return Some((entry, little));
        }
    }
    None
}

fn read_u16(file: &[u8], at: usize, little: bool) -> Option<u16> {
    let bytes: [u8; 2] = file.get(at..at.checked_add(2)?)?.try_into().ok()?;
    Some(if little {
        u16::from_le_bytes(bytes)
    } else {
        u16::from_be_bytes(bytes)
    })
}

fn read_u32(file: &[u8], at: usize, little: bool) -> Option<u32> {
    let bytes: [u8; 4] = file.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(if little {
        u32::from_le_bytes(bytes)
    } else {
        u32::from_be_bytes(bytes)
    })
}
