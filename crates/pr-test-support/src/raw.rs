//! Pure byte builders for the RAW tests (D48 of `contracts/README.md`): port of
//! `tests/PhotoReview.Imaging.Tests/Raw/Tiff/TiffBytes.cs`, `Raw/SyntheticRawBuilder.cs` (TIFF, ISO-BMFF and RAF parts) and
//! `Robustness/BinaryFuzz.cs` (`Mutants`). Owned by W5-02a (first TIFF porter); later RAW units only consume them.
//! Nothing here is production code and nothing touches the file system.
//!
//! The fuzz corpus is NOT byte-identical to C# (C# `System.Random` has no Rust twin): every family is the same, the random
//! draws come from [`XorShift64`]. AGENTS.md "Tests": a corpus only has to be reproducible, not equal to the C# one.

use serde_json::Value;

// ---------------------------------------------------------------- TiffBytes

/// One 12-byte IFD entry (C# `TiffBytes.Entry`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub tag: u16,
    pub ty: u16,
    pub count: u32,
    pub value: u32,
}

/// Inline SHORT x 1: the value is left-justified in the value field (C# `TiffBytes.Short`).
#[must_use]
pub fn short(tag: u16, value: u32) -> Entry {
    Entry {
        tag,
        ty: 3,
        count: 1,
        value,
    }
}

/// Inline LONG x 1 (C# `TiffBytes.Long`).
#[must_use]
pub fn long(tag: u16, value: u32) -> Entry {
    Entry {
        tag,
        ty: 4,
        count: 1,
        value,
    }
}

/// Out-of-line entry (array, rational, blob) whose data lives at `offset` (C# `TiffBytes.At`).
#[must_use]
pub fn at(tag: u16, ty: u16, count: u32, offset: u32) -> Entry {
    Entry {
        tag,
        ty,
        count,
        value: offset,
    }
}

/// Lays out a TIFF-family file byte by byte at explicit offsets, so hostile structures can be written exactly.
/// Every method consumes and returns the builder (C# fluent style).
#[derive(Debug, Clone)]
pub struct TiffBytes {
    little: bool,
    buf: Vec<u8>,
}

impl TiffBytes {
    /// Zero-filled block of `size` bytes in the given byte order.
    #[must_use]
    pub fn new(little: bool, size: usize) -> Self {
        Self {
            little,
            buf: vec![0; size],
        }
    }

    /// `II*\0` (little) or `MM\0*` (big) at offset 0, then the IFD0 offset at 4.
    #[must_use]
    pub fn header(self, ifd0: u32) -> Self {
        let magic: &[u8] = if self.little { b"II*\0" } else { b"MM\0*" };
        self.put(0, magic).u32(4, ifd0)
    }

    /// Copies raw bytes at `pos`.
    #[must_use]
    pub fn put(mut self, pos: usize, bytes: &[u8]) -> Self {
        self.buf[pos..pos + bytes.len()].copy_from_slice(bytes);
        self
    }

    /// A `u16` in the builder's byte order at `pos`.
    #[must_use]
    pub fn u16(self, pos: usize, value: u16) -> Self {
        let raw = if self.little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        self.put(pos, &raw)
    }

    /// A `u32` in the builder's byte order at `pos`.
    #[must_use]
    pub fn u32(self, pos: usize, value: u32) -> Self {
        let raw = if self.little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        self.put(pos, &raw)
    }

    /// Writes an IFD of `entries` at `pos`, then the next-IFD pointer. SHORT x 1 values are left-justified (C# `Ifd`).
    #[must_use]
    pub fn ifd(self, pos: usize, next: u32, entries: &[Entry]) -> Self {
        let count = u16::try_from(entries.len()).expect("entry count fits u16");
        let mut me = self.u16(pos, count);
        for (i, e) in entries.iter().enumerate() {
            let base = pos + 2 + i * 12;
            me = me.u16(base, e.tag).u16(base + 2, e.ty).u32(base + 4, e.count);
            me = if e.ty == 3 && e.count == 1 {
                me.u16(base + 8, u16::try_from(e.value).expect("SHORT value fits u16"))
            } else {
                me.u32(base + 8, e.value)
            };
        }
        me.u32(pos + 2 + entries.len() * 12, next)
    }

    /// The bytes written so far (the whole block).
    #[must_use]
    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }
}

// ---------------------------------------------------------------- SyntheticRawBuilder

/// A minimal valid baseline JPEG (SOI, APP0 JFIF, DQT, SOF0, DHT, SOS, six data bytes, EOI). C# `CreateMinimalJpeg`.
///
/// # Panics
/// When a dimension does not fit in 16 bits (test input only).
#[must_use]
pub fn create_minimal_jpeg(width: i32, height: i32) -> Vec<u8> {
    let h = u16::try_from(height).expect("height fits u16").to_be_bytes();
    let w = u16::try_from(width).expect("width fits u16").to_be_bytes();
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&[0xFF, 0xD8]); // SOI
    out.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x10]); // APP0
    out.extend_from_slice(b"JFIF\0");
    out.extend_from_slice(&[0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00]);
    out.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]); // DQT
    out.extend(std::iter::repeat_n(16u8, 64));
    out.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]); // SOF0, precision 8
    out.extend_from_slice(&h);
    out.extend_from_slice(&w);
    out.push(3); // components
    out.extend_from_slice(&[0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
    out.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x1F, 0x00]); // DHT
    out.extend_from_slice(&[
        0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]);
    out.extend_from_slice(&[0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B]);
    out.extend_from_slice(&[
        0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3F, 0x00,
    ]); // SOS
    out.extend_from_slice(&[0xAA, 0x55, 0xAA, 0x55, 0xAA, 0x55]); // data: no bare FF
    out.extend_from_slice(&[0xFF, 0xD9]); // EOI
    out
}

/// A TIFF container (little or big endian) with IFD0 at 8: Orientation (0x0112), JPEGInterchangeFormat (0x0201) and
/// JPEGInterchangeFormatLength (0x0202), next pointer 0, then the JPEG right after it (C# `BuildTiff`).
/// `jpeg` defaults to `create_minimal_jpeg(640, 480)`.
///
/// # Panics
/// When `jpeg` is longer than `u32::MAX` (test input only).
#[must_use]
pub fn build_tiff(little: bool, jpeg: Option<&[u8]>, orientation: u16, magic: u16) -> Vec<u8> {
    let default;
    let jpeg = match jpeg {
        Some(bytes) => bytes,
        None => {
            default = create_minimal_jpeg(640, 480);
            &default
        }
    };
    let jpeg_offset: usize = 8 + 2 + 3 * 12 + 4;
    let len = u32::try_from(jpeg.len()).expect("jpeg fits u32");
    let jpeg_offset_u32 = u32::try_from(jpeg_offset).expect("offset fits u32");
    let byte_order: &[u8] = if little { b"II" } else { b"MM" };
    let builder = TiffBytes::new(little, jpeg_offset + jpeg.len())
        .put(0, byte_order)
        .u16(2, magic)
        .u32(4, 8)
        .ifd(
            8,
            0,
            &[
                short(0x0112, u32::from(orientation)),
                long(0x0201, jpeg_offset_u32),
                long(0x0202, len),
            ],
        )
        .put(jpeg_offset, jpeg);
    builder.into_vec()
}

/// A minimal ISO-BMFF (CR3-style) container: an `ftyp` box (`crx `) and a `prvw` box holding the JPEG (C# `BuildIsoBmff`).
#[must_use]
pub fn build_iso_bmff(jpeg: Option<&[u8]>) -> Vec<u8> {
    let default;
    let jpeg = match jpeg {
        Some(bytes) => bytes,
        None => {
            default = create_minimal_jpeg(800, 600);
            &default
        }
    };
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&16u32.to_be_bytes());
    out.extend_from_slice(b"ftyp");
    out.extend_from_slice(b"crx ");
    out.extend_from_slice(&1u32.to_be_bytes());
    let prvw_len = u32::try_from(8 + jpeg.len()).expect("prvw fits u32");
    out.extend_from_slice(&prvw_len.to_be_bytes());
    out.extend_from_slice(b"prvw");
    out.extend_from_slice(jpeg);
    out
}

/// A minimal Fujifilm RAF header: magic, version, camera id, the JPEG offset (160) and length at 84 / 88 (big endian),
/// then the JPEG (C# `BuildRaf`). `jpeg` defaults to `create_minimal_jpeg(1024, 768)`.
#[must_use]
pub fn build_raf(jpeg: Option<&[u8]>) -> Vec<u8> {
    let default;
    let jpeg = match jpeg {
        Some(bytes) => bytes,
        None => {
            default = create_minimal_jpeg(1024, 768);
            &default
        }
    };
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"FUJIFILMCCD-RAW ");
    out.extend_from_slice(b"0201");
    out.extend_from_slice(b"FF-TEST\0");
    out.resize(84, 0);
    let jpeg_offset: u32 = 160;
    out.extend_from_slice(&jpeg_offset.to_be_bytes());
    out.extend_from_slice(&u32::try_from(jpeg.len()).expect("jpeg fits u32").to_be_bytes());
    out.resize(usize::try_from(jpeg_offset).expect("offset fits usize"), 0);
    out.extend_from_slice(jpeg);
    out
}

// ---------------------------------------------------------------- golden spec nodes

/// Decodes a golden input byte node: `{"hex": "..."}` or `{"len": n, "patches": [{"at": i, "hex": "..."}]}` (zero filled).
/// Port of the W4-03a `data` node decoder (`pr-imaging` tiff tests), shared from here by the RAW goldens.
///
/// # Panics
/// On a malformed node (test input only).
#[must_use]
pub fn golden_bytes(node: &Value) -> Vec<u8> {
    if let Some(text) = node["hex"].as_str() {
        return from_hex(text);
    }
    let len = usize::try_from(node["len"].as_u64().expect("data len")).expect("len fits usize");
    let mut bytes = vec![0u8; len];
    for patch in node["patches"].as_array().into_iter().flatten() {
        let at = usize::try_from(patch["at"].as_u64().expect("patch at")).expect("at fits usize");
        let raw = from_hex(patch["hex"].as_str().expect("patch hex"));
        bytes[at..at + raw.len()].copy_from_slice(&raw);
    }
    bytes
}

/// Decodes an even-length hex string (upper or lower case).
///
/// # Panics
/// On an odd length or a non-hex digit (test input only).
#[must_use]
pub fn from_hex(text: &str) -> Vec<u8> {
    assert!(text.len() % 2 == 0, "odd hex length: {text}");
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex digit"))
        .collect()
}

// ---------------------------------------------------------------- BinaryFuzz.Mutants

/// Deterministic xorshift64 generator (no `rand` crate, AGENTS.md dependency rules).
#[derive(Debug, Clone)]
pub struct XorShift64(u64);

impl XorShift64 {
    /// A zero seed would stick at zero, so it is replaced by a fixed non-zero constant.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self(if seed == 0 { 0x2545_F491_4F6C_DD1D } else { seed })
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform-enough value in `0..bound` (`bound` must be non-zero).
    pub fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next_u64() % u64::try_from(bound).expect("bound fits u64")).expect("index fits usize")
    }
}

/// One labelled mutant. The label names the mutation and its position, so a failure reproduces from the message alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    pub label: String,
    pub bytes: Vec<u8>,
}

/// Cap on the cases one systematic family yields per seed (C# `BinaryFuzz.FamilyCap`).
const FAMILY_CAP: usize = 4_000;

/// The seed itself first, then truncations, length-field extremes (u32 / u16, both byte orders) and `random_count` seeded
/// random mutants (bit flips, byte stores, splices). Deterministic for a given `rng_seed` (C# `BinaryFuzz.Mutants`).
#[must_use]
pub fn mutants(seed: &[u8], rng_seed: u64, random_count: usize) -> Vec<Case> {
    let mut out = vec![Case {
        label: "seed".to_owned(),
        bytes: seed.to_vec(),
    }];

    // 1. Truncation: every offset for a small file, otherwise the dense head, the dense tail and an even spread.
    for keep in truncation_offsets(seed.len()) {
        out.push(Case {
            label: format!("truncate@{keep}"),
            bytes: seed[..keep].to_vec(),
        });
    }

    // 2. Field overwrite with extremes at strided positions, in both byte orders.
    let extremes32: [u32; 5] = [0, 0x7FFF_FFFF, 0xFFFF_FFFF, 0x8000_0000, 1];
    let extremes16: [u16; 4] = [0, 0x7FFF, 0xFFFF, 0x8000];
    for pos in positions(seed.len(), FAMILY_CAP / (extremes32.len() * 2 + extremes16.len() * 2)) {
        if pos + 4 <= seed.len() {
            for value in extremes32 {
                let mut le = seed.to_vec();
                le[pos..pos + 4].copy_from_slice(&value.to_le_bytes());
                out.push(Case {
                    label: format!("u32le@{pos}={value:08X}"),
                    bytes: le,
                });
                let mut be = seed.to_vec();
                be[pos..pos + 4].copy_from_slice(&value.to_be_bytes());
                out.push(Case {
                    label: format!("u32be@{pos}={value:08X}"),
                    bytes: be,
                });
            }
        }
        if pos + 2 <= seed.len() {
            for value in extremes16 {
                let mut le = seed.to_vec();
                le[pos..pos + 2].copy_from_slice(&value.to_le_bytes());
                out.push(Case {
                    label: format!("u16le@{pos}={value:04X}"),
                    bytes: le,
                });
                let mut be = seed.to_vec();
                be[pos..pos + 2].copy_from_slice(&value.to_be_bytes());
                out.push(Case {
                    label: format!("u16be@{pos}={value:04X}"),
                    bytes: be,
                });
            }
        }
    }

    // 3. Seeded random: bit flips, byte stores, overwrite splices and insert splices.
    let mut rng = XorShift64::new(rng_seed);
    for _ in 0..random_count {
        let mut data = seed.to_vec();
        let label = match rng.below(4) {
            0 => {
                let flips = 1 + rng.below(4);
                let mut label = String::from("bitflip");
                for _ in 0..flips {
                    let p = rng.below(data.len());
                    let bit = rng.below(8);
                    data[p] ^= 1u8 << bit;
                    label.push_str(&format!(" {p}.{bit}"));
                }
                label
            }
            1 => {
                let p = rng.below(data.len());
                let b = u8::try_from(rng.below(256)).expect("byte");
                data[p] = b;
                format!("store@{p}={b:02X}")
            }
            2 => {
                let from = rng.below(data.len());
                let len = 1 + rng.below(64usize.min(data.len() - from));
                let to = rng.below(data.len());
                let n = len.min(data.len() - to);
                data[to..to + n].copy_from_slice(&seed[from..from + n]);
                format!("splice-over from={from} len={n} to={to}")
            }
            _ => {
                let from = rng.below(data.len());
                let len = 1 + rng.below(64usize.min(data.len() - from));
                let to = rng.below(data.len() + 1);
                let mut spliced = data[..to].to_vec();
                spliced.extend_from_slice(&seed[from..from + len]);
                spliced.extend_from_slice(&data[to..]);
                data = spliced;
                format!("splice-insert from={from} len={len} to={to}")
            }
        };
        out.push(Case { label, bytes: data });
    }
    out
}

fn truncation_offsets(length: usize) -> Vec<usize> {
    if length <= FAMILY_CAP {
        return (0..length).collect();
    }
    let mut seen = std::collections::BTreeSet::new();
    seen.extend(0..512);
    seen.extend(length - 256..length);
    let step = (length / (FAMILY_CAP - 768)).max(1);
    seen.extend((512..length).step_by(step));
    seen.into_iter().collect()
}

fn positions(length: usize, cap: usize) -> Vec<usize> {
    let step = length.div_ceil(cap.max(1)).max(1);
    (0..length).step_by(step).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xorshift_is_deterministic_and_never_zero_state() {
        let mut a = XorShift64::new(7);
        let mut b = XorShift64::new(7);
        assert_eq!(a.next_u64(), b.next_u64());
        assert_ne!(XorShift64::new(0).next_u64(), 0);
    }

    #[test]
    fn from_hex_decodes_both_cases() {
        assert_eq!(from_hex("49fF2a"), vec![0x49, 0xFF, 0x2A]);
    }
}
