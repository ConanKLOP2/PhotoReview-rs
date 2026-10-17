//! Port of `PhotoReview.Imaging.Raw/{RawContainerReaderRegistry, RawJpegIccProfile}.cs` (C# baseline d92d6aae).
//!
//! * [`RawContainerReaderRegistry`] picks the first registered [`RawContainerReader`] whose `can_read` accepts the first
//!   64 bytes of a file plus its extension. Pure routing, no I/O.
//! * [`ensure_adobe_rgb_profile`] gives a RAW preview that `PreviewSelector` tagged `AdobeRgb` (EXIF interoperability index
//!   "R03") but that carries no ICC profile the bundled CC0 Adobe-compatible profile, as one APP2 segment right after SOI.
//!
//! Both live in one file (target path fixed by the roster). Everything here is total on hostile input: the JPEG walk is
//! bounds checked and never indexes past the end; the only errors are the two `InvalidData` cases the C# throws.

use std::fmt;

use crate::RawHeaderError;
use crate::contracts::RawContainerReader;
use crate::readers::arw::ArwContainerReader;
use crate::readers::cr2::Cr2ContainerReader;
use crate::readers::cr3::Cr3ContainerReader;
use crate::readers::dng::DngContainerReader;
use crate::readers::nef::NefContainerReader;
use crate::readers::orf::OrfContainerReader;
use crate::readers::raf::RafContainerReader;
use crate::readers::rw2::Rw2ContainerReader;

/// The reader names of the default registry in registration order (the C# constructor's list). Not in C#: it lets the
/// golden test (`raw-registry-find`, case `default-registration-order`) pin the order and map
/// [`RawContainerReaderRegistry::find_reader_index`] back to a name without adding a `name()` method to the trait.
/// Order matters only when two readers accept the same input; today each reader owns one extension, so it is
/// observable only through `find_reader_index`.
pub const DEFAULT_READER_ORDER: [&str; 8] = ["Cr2", "Nef", "Arw", "Dng", "Orf", "Rw2", "Cr3", "Raf"];

/// `RawContainerReaderRegistry`: the readers are tried in registration order and the first that accepts wins. Readers are
/// stateless and `Send + Sync`, so the registry is too (the decoder shares one across threads).
pub struct RawContainerReaderRegistry {
    readers: Vec<Box<dyn RawContainerReader>>,
}

impl fmt::Debug for RawContainerReaderRegistry {
    /// `dyn RawContainerReader` has no `Debug`: print the reader count only.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawContainerReaderRegistry")
            .field("reader_count", &self.readers.len())
            .finish()
    }
}

impl Default for RawContainerReaderRegistry {
    /// Same as [`RawContainerReaderRegistry::new`].
    fn default() -> Self {
        Self::new()
    }
}

impl RawContainerReaderRegistry {
    /// C# parameterless constructor: the standard readers in [`DEFAULT_READER_ORDER`] (`Cr2ContainerReader`,
    /// `NefContainerReader`, `ArwContainerReader`, `DngContainerReader`, `OrfContainerReader`, `Rw2ContainerReader`,
    /// `Cr3ContainerReader`, `RafContainerReader`: W5-03..W5-10, all unit structs, `Default`). NRW and PEF are
    /// deliberately not registered (reserved formats): `find_reader(.., ".nrw")` is `None`.
    pub fn new() -> Self {
        Self::with_readers([
            Box::new(Cr2ContainerReader) as Box<dyn RawContainerReader>,
            Box::new(NefContainerReader),
            Box::new(ArwContainerReader),
            Box::new(DngContainerReader),
            Box::new(OrfContainerReader),
            Box::new(Rw2ContainerReader),
            Box::new(Cr3ContainerReader),
            Box::new(RafContainerReader),
        ])
    }

    /// C# `RawContainerReaderRegistry(IEnumerable<IRawContainerReader>)`: exactly the given readers, in the given order
    /// (an empty list is allowed and finds nothing; duplicates are kept, the first one wins). C# `ArgumentNullException`
    /// for a null list has no Rust twin.
    pub fn with_readers(readers: impl IntoIterator<Item = Box<dyn RawContainerReader>>) -> Self {
        Self {
            readers: readers.into_iter().collect(),
        }
    }

    /// Number of registered readers (not in C#; used by the tests).
    pub fn len(&self) -> usize {
        self.readers.len()
    }

    /// True when no reader is registered.
    pub fn is_empty(&self) -> bool {
        self.readers.is_empty()
    }

    /// `FindReader`: the first registered reader whose `can_read(first_64_bytes, extension)` is true, else `None`.
    /// `first_64_bytes` is passed through untouched (a shorter slice is the reader's business; the registry neither pads nor
    /// truncates), `extension` includes the dot and is compared by the readers (OrdinalIgnoreCase). Returns a borrow of the
    /// registered reader (C# `Assert.Same`), no clone.
    pub fn find_reader(&self, first_64_bytes: &[u8], extension: &str) -> Option<&dyn RawContainerReader> {
        self.readers
            .iter()
            .find(|reader| reader.can_read(first_64_bytes, extension))
            .map(Box::as_ref)
    }

    /// Same scan as [`find_reader`](Self::find_reader), returning the zero-based position of the winner (not in C#; the golden
    /// maps it to a reader name through [`DEFAULT_READER_ORDER`]).
    pub fn find_reader_index(&self, first_64_bytes: &[u8], extension: &str) -> Option<usize> {
        self.readers
            .iter()
            .position(|reader| reader.can_read(first_64_bytes, extension))
    }
}

/// The 12-byte ICC APP2 identifier `"ICC_PROFILE\0"` (C# `IccIdentifier`).
pub const ICC_IDENTIFIER: [u8; 12] = *b"ICC_PROFILE\0";

/// The bundled CC0 Adobe-compatible ICC profile (`AdobeCompat-v2.icc`, 374 bytes; C# `s_adobeRgbProfile`). Rust twin of the
/// C# embedded resource: `include_bytes!("../assets/AdobeCompat-v2.icc")` (the lead copies the file, byte for byte, from
/// `contracts/drafts-w5/W5-11a/assets/AdobeCompat-v2.icc`; golden case `bundled-profile-identity` pins length and FNV-1a 64).
/// There is no lazy load and no "resource is missing" error: a missing file is a compile error.
pub fn adobe_rgb_profile() -> &'static [u8] {
    include_bytes!("../assets/AdobeCompat-v2.icc")
}

/// `RawJpegIccProfile.EnsureAdobeRgbProfile`: `jpeg` unchanged (the same allocation: C# `Assert.Same`) when it already
/// carries an ICC profile ([`has_embedded_icc`]); otherwise a new buffer
/// `FF D8 | FF E2 | len_hi len_lo | "ICC_PROFILE\0" | 01 01 | profile | jpeg[2..]` with `len = 16 + profile.len()` (big endian,
/// 390 for the bundled profile). The input is taken by value because the caller (W5-12 `RawDecoder`) owns the preview
/// bytes: the tagged path costs nothing; there is no way to mutate a borrowed input (C# test `DoesNotMutateTheInput` holds
/// by construction, the owned input is only read).
///
/// Errors (`RawHeaderError::InvalidData`, no inner io error): `jpeg` shorter than 2 bytes or not starting `FF D8` and not
/// already tagged (`has_embedded_icc` is false for such input) => message `"Adobe RGB preview is not a valid JPEG stream."`.
/// Order of checks: tagged => return; then the SOI check; then the profile length check.
pub fn ensure_adobe_rgb_profile(jpeg: Vec<u8>) -> Result<Vec<u8>, RawHeaderError> {
    if has_embedded_icc(&jpeg) {
        return Ok(jpeg);
    }
    if jpeg.len() < 2 || jpeg.first() != Some(&0xFF) || jpeg.get(1) != Some(&0xD8) {
        return Err(RawHeaderError::invalid_data(
            "Adobe RGB preview is not a valid JPEG stream.",
        ));
    }
    insert_icc_app2(&jpeg, adobe_rgb_profile())
}

/// The injection step of [`ensure_adobe_rgb_profile`] with an explicit profile (C# has it inline; split so the unreachable
/// size guard is testable with a 70 000 byte profile): `jpeg` must start with `FF D8` (checked by the caller).
/// `16 + profile.len() > 65535` => `InvalidData` `"Adobe RGB ICC profile exceeds JPEG APP2 limits."`. Overflow-free: C#
/// `checked(..)` can only throw `OverflowException` for absurd sizes that cannot happen with a `Vec`, use `checked_add`
/// and map it to the same error rather than panic.
pub fn insert_icc_app2(jpeg: &[u8], profile: &[u8]) -> Result<Vec<u8>, RawHeaderError> {
    let segment_length = profile
        .len()
        .checked_add(16)
        .and_then(|length| u16::try_from(length).ok())
        .ok_or_else(|| RawHeaderError::invalid_data("Adobe RGB ICC profile exceeds JPEG APP2 limits."))?;
    let capacity = jpeg
        .len()
        .checked_add(18)
        .and_then(|length| length.checked_add(profile.len()))
        .ok_or_else(|| RawHeaderError::invalid_data("Adobe RGB preview is too large."))?;

    let mut result = Vec::with_capacity(capacity);
    result.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xE2]);
    result.extend_from_slice(&segment_length.to_be_bytes());
    result.extend_from_slice(&ICC_IDENTIFIER);
    result.extend_from_slice(&[1, 1]);
    result.extend_from_slice(profile);
    if let Some(tail) = jpeg.get(2..) {
        result.extend_from_slice(tail);
    }
    Ok(result)
}

/// `HasEmbeddedIcc`: true when the JPEG header area holds an APP2 segment whose payload starts with [`ICC_IDENTIFIER`].
/// Walks markers from offset 2 while `offset + 4 <= len`:
/// * the byte at `offset` must be `FF`, else `false`; skip the run of `FF` fill bytes (reaching the end => `false`);
/// * the next byte is the marker; `DA` (SOS) or `D9` (EOI) => `false` (entropy-coded data is not scanned);
/// * `D8`, `01` and `D0..=D7` are stand-alone (no length): `continue` (note the loop condition is re-checked, so such a
///   marker as the last bytes ends the walk);
/// * otherwise two length bytes (big endian, include themselves) must fit (`offset + 2 <= len`, else `false`); `length < 2`
///   or `offset + length > len` => `false` (no throw, not tagged); an APP2 (`E2`) payload of at least 12 bytes starting with
///   the identifier => `true`; else `offset += length`.
///
/// `offset` arithmetic in `usize` after the explicit bounds checks above; never index before checking (hardening tests:
/// stream ending inside fill bytes or right after a marker byte). Input shorter than 2 bytes or without `FF D8` => `false`.
pub fn has_embedded_icc(jpeg: &[u8]) -> bool {
    if jpeg.len() < 2 || jpeg.first() != Some(&0xFF) || jpeg.get(1) != Some(&0xD8) {
        return false;
    }

    let mut offset = 2_usize;
    while offset <= jpeg.len().saturating_sub(4) {
        if jpeg.get(offset) != Some(&0xFF) {
            return false;
        }

        while jpeg.get(offset) == Some(&0xFF) {
            offset += 1;
        }

        let Some(&marker) = jpeg.get(offset) else {
            return false;
        };
        offset += 1;

        if marker == 0xDA || marker == 0xD9 {
            return false;
        }
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }

        let Some(length_bytes) = offset.checked_add(2).and_then(|end| jpeg.get(offset..end)) else {
            return false;
        };
        let length = usize::from(u16::from_be_bytes([length_bytes[0], length_bytes[1]]));
        let Some(segment_end) = offset.checked_add(length) else {
            return false;
        };
        if length < 2 || segment_end > jpeg.len() {
            return false;
        }

        let payload_start = offset + 2;
        if marker == 0xE2 && length >= 14 {
            let Some(identifier_end) = payload_start.checked_add(ICC_IDENTIFIER.len()) else {
                return false;
            };
            if jpeg.get(payload_start..identifier_end) == Some(ICC_IDENTIFIER.as_slice()) {
                return true;
            }
        }
        offset = segment_end;
    }
    false
}
