//! W5-10b read-parity tests: CMT1 admission bounds and the zero-chunk-offset probe (kill tests for boundary mutants in cr3.rs).
use std::io;

use pr_core::sync::CancelToken;
use pr_imaging_raw::readers::cr3::Cr3ContainerReader;
use pr_imaging_raw::{InMemoryRawHeaderSource, RawContainerReader, RawHeaderError, RawHeaderSource};

const CANON_UUID: [u8; 16] = [
    0x85, 0xC0, 0xB6, 0x87, 0x82, 0x0F, 0x11, 0xE0, 0x81, 0x11, 0xF4, 0xCE, 0x46, 0x2B, 0x6A, 0x48,
];

fn bx(kind: &str, payload: &[u8]) -> Vec<u8> {
    let mut out = u32::try_from(8 + payload.len()).unwrap().to_be_bytes().to_vec();
    out.extend_from_slice(kind.as_bytes());
    out.extend_from_slice(payload);
    out
}

fn ftyp() -> Vec<u8> {
    let mut p = b"crx ".to_vec();
    p.extend_from_slice(&1u32.to_be_bytes());
    p.extend_from_slice(b"crx ");
    bx("ftyp", &p)
}

/// `ftyp + moov(canon uuid(CMT1 payload)) + free(64 zero bytes)`; returns the file and the CMT1 payload offset.
fn file_with_cmt1(payload: &[u8]) -> (Vec<u8>, i64) {
    let mut uuid_payload = CANON_UUID.to_vec();
    uuid_payload.extend_from_slice(&bx("CMT1", payload));
    let file = [ftyp(), bx("moov", &bx("uuid", &uuid_payload)), bx("free", &[0u8; 64])].concat();
    let pos = file.windows(4).position(|w| w == b"CMT1").unwrap();
    (file, i64::try_from(pos + 4).unwrap())
}

/// LE TIFF of `size` bytes with IFD0 pointer `ifd0` and zeros elsewhere (entry count 0 at the IFD).
fn tiff(size: usize, ifd0: u32) -> Vec<u8> {
    let mut v = vec![0u8; size];
    v[..4].copy_from_slice(b"II*\0");
    v[4..8].copy_from_slice(&ifd0.to_le_bytes());
    v
}

struct FailAt {
    inner: InMemoryRawHeaderSource,
    offset: i64,
    count: i32,
    hit: bool,
}

impl RawHeaderSource for FailAt {
    fn length(&self) -> i64 {
        self.inner.length()
    }
    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        if offset == self.offset && (self.count == 0 || count == self.count) {
            self.hit = true;
            return Err(RawHeaderError::invalid_data_io(
                "injected",
                io::Error::other("injected"),
            ));
        }
        self.inner.read(offset, count)
    }
}

/// Returns (result is Ok, a read happened at `offset`).
fn read_failing_at(file: &[u8], offset: i64, count: i32) -> (bool, bool) {
    let mut s = FailAt {
        inner: InMemoryRawHeaderSource::new(file.to_vec()),
        offset,
        count,
        hit: false,
    };
    let r = Cr3ContainerReader.read(&mut s, &CancelToken::none());
    if let Err(e) = &r {
        assert!(e.is_io_failure(), "only the injected failure may escape: {e}");
    }
    (r.is_ok(), s.hit)
}

#[test]
fn cmt1_payload_of_16_bytes_reads_its_tiff_header_but_15_does_not() {
    for (size, expect_read) in [(15usize, false), (16, true), (17, true)] {
        let (file, at) = file_with_cmt1(&tiff(size, 8));
        let (ok, hit) = read_failing_at(&file, at, 0);
        assert_eq!(hit, expect_read, "payload {size}: header read at payload start");
        assert_eq!(ok, !expect_read, "payload {size}");
    }
}

#[test]
fn cmt1_ifd0_pointer_is_followed_up_to_payload_minus_two_only() {
    let size = 24usize;
    for (ifd0, expect_read) in [
        (22u32, true),
        (21, true),
        (23, false),
        (24, false),
        (25, false),
        (26, false),
        (27, false),
    ] {
        let (file, at) = file_with_cmt1(&tiff(size, ifd0));
        let (ok, hit) = read_failing_at(&file, at + i64::from(ifd0), 2);
        assert_eq!(hit, expect_read, "ifd0 {ifd0}: IFD read");
        assert_eq!(ok, !expect_read, "ifd0 {ifd0}");
    }
}

struct Recording {
    inner: InMemoryRawHeaderSource,
    reads: Vec<(i64, i32)>,
}

impl RawHeaderSource for Recording {
    fn length(&self) -> i64 {
        self.inner.length()
    }
    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        self.reads.push((offset, count));
        self.inner.read(offset, count)
    }
}

fn track_file(uniform: u32, chunk_offset: u32, pad: usize) -> Vec<u8> {
    let mut stsz = vec![0u8; 4];
    stsz.extend_from_slice(&uniform.to_be_bytes());
    stsz.extend_from_slice(&0u32.to_be_bytes());
    let mut stco = vec![0u8; 4];
    stco.extend_from_slice(&1u32.to_be_bytes());
    stco.extend_from_slice(&chunk_offset.to_be_bytes());
    let stbl = bx("stbl", &[bx("stsz", &stsz), bx("stco", &stco)].concat());
    let trak = bx("trak", &bx("mdia", &bx("minf", &stbl)));
    [ftyp(), bx("moov", &trak), vec![0u8; pad]].concat()
}

#[test]
fn zero_chunk_offset_never_probes_the_track_jpeg_at_file_start() {
    let file = track_file(100, 0, 200);
    let mut s = Recording {
        inner: InMemoryRawHeaderSource::new(file),
        reads: Vec::new(),
    };
    let info = Cr3ContainerReader.read(&mut s, &CancelToken::none()).unwrap();
    assert!(info.previews.is_empty());
    assert!(
        !s.reads.iter().any(|&(o, c)| o == 0 && c == 2),
        "probe read at offset 0: {:?}",
        s.reads
    );
}
